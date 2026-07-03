//! Background review workflow infrastructure
//!
//! Provides the global `PendingReviewRegistry` and Curator auto-run helper.
//! The actual review execution lives in `crate::review`.

use super::curator::{Curator, CuratorConfig};
use super::skill_registry::{default_path as skills_default_path, SkillRegistry};
use agent::ReactBuildConfig;
use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tracing::{info, warn};

/// Global registry to track pending background review tasks.
static PENDING_REVIEWS: std::sync::OnceLock<Arc<PendingReviewRegistry>> =
    std::sync::OnceLock::new();

fn pending_reviews() -> &'static Arc<PendingReviewRegistry> {
    PENDING_REVIEWS.get_or_init(|| Arc::new(PendingReviewRegistry::new()))
}

/// Access the global `PendingReviewRegistry`.
///
/// Initialized lazily on first call. Exposed for cross-crate callers
/// (e.g. `apps/acp/src/review_runner.rs`) that need per-session review
/// deduplication across the whole process.
pub fn global_registry() -> &'static Arc<PendingReviewRegistry> {
    pending_reviews()
}

/// Registry of pending background review handles.
///
/// Two complementary responsibilities:
/// - `push` / `wait_all` track spawned tokio task handles for graceful shutdown.
/// - `try_acquire` enforces per-session deduplication so a single session
///   cannot stack overlapping reviews when prompts land faster than reviews
///   complete.
pub struct PendingReviewRegistry {
    handles: StdMutex<Vec<tokio::task::JoinHandle<()>>>,
    sessions: StdMutex<HashSet<String>>,
}

/// RAII guard for a per-session review slot.
///
/// Holds the slot for the lifetime of the guard and releases it on drop.
/// Move into the spawned task (or OS thread) so the slot lives as long as
/// the review itself.
pub struct ReviewGuard {
    registry: Arc<PendingReviewRegistry>,
    session_id: String,
}

impl Drop for ReviewGuard {
    fn drop(&mut self) {
        match self.registry.sessions.lock() {
            Ok(mut sessions) => {
                sessions.remove(&self.session_id);
            }
            Err(poisoned) => {
                let mut sessions = poisoned.into_inner();
                sessions.remove(&self.session_id);
            }
        }
    }
}

impl PendingReviewRegistry {
    fn new() -> Self {
        Self {
            handles: StdMutex::new(Vec::new()),
            sessions: StdMutex::new(HashSet::new()),
        }
    }

    pub fn push(&self, handle: tokio::task::JoinHandle<()>) {
        let mut handles = self.handles.lock().unwrap();
        handles.push(handle);
    }

    /// Try to acquire a per-session review slot.
    ///
    /// Returns `Some(guard)` if no review is currently in flight for
    /// `session_id`. The guard holds the slot until dropped — move it into
    /// the spawned task so the slot is released when the review completes.
    ///
    /// Returns `None` if a review is already running for that session;
    /// the caller should skip the spawn (and ideally log it for observability).
    pub fn try_acquire(self: &Arc<Self>, session_id: String) -> Option<ReviewGuard> {
        let mut sessions = match self.sessions.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if !sessions.insert(session_id.clone()) {
            return None;
        }
        Some(ReviewGuard {
            registry: self.clone(),
            session_id,
        })
    }

    /// Number of sessions with a review currently in flight.
    pub fn active_sessions(&self) -> usize {
        match self.sessions.lock() {
            Ok(g) => g.len(),
            Err(p) => p.into_inner().len(),
        }
    }

    pub async fn wait_all(&self) -> usize {
        let handles: Vec<_> = {
            let mut handles = self.handles.lock().unwrap();
            handles.drain(..).collect()
        };
        let count = handles.len();
        if count == 0 {
            return 0;
        }

        info!("Waiting for {} background review(s) to complete...", count);

        const WAIT_TIMEOUT: Duration = Duration::from_secs(60);
        for handle in handles {
            match tokio::time::timeout(WAIT_TIMEOUT, handle).await {
                Ok(_) => {}
                Err(_) => warn!(
                    timeout = ?WAIT_TIMEOUT,
                    "Background review task did not finish in time, abandoning"
                ),
            }
        }

        info!("All {} background review(s) completed.", count);
        count
    }
}

/// Callback type for review output notifications.
pub type ReviewOutputFn = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
pub struct BackgroundReviewCallbacks {
    pub on_output: Option<ReviewOutputFn>,
    pub on_review_complete: Option<ReviewOutputFn>,
}

pub async fn wait_for_pending_reviews() -> usize {
    pending_reviews().wait_all().await
}

pub fn run_curator_if_needed(
    skills_path: &std::path::Path,
    curator_config: &CuratorConfig,
) -> Result<(), String> {
    let skills = SkillRegistry::new(skills_path);
    let state_path = skills_path.join("curator").join("state.json");
    let curator = Curator::new(skills, curator_config.clone()).with_state_path(state_path);

    if !curator.should_run(None) {
        return Ok(());
    }

    let report = curator.run(false).map_err(|e| format!("{:?}", e))?;

    if !report.stale.is_empty() || !report.archived.is_empty() || !report.overlapping.is_empty() {
        info!(
            "Curator: {} active, {} stale, {} archived, {} overlapping",
            report.active,
            report.stale.len(),
            report.archived.len(),
            report.overlapping.len(),
        );
    }

    Ok(())
}

/// Run the curator's LLM consolidation pass if the conditions are met.
///
/// This is the LLM-driven umbrella-building pass (Phase I of the curator
/// alignment plan). It checks `should_run()`, builds the prompt from active
/// skills, invokes the LLM in a multi-turn loop with `skill_manage` tools,
/// and classifies the results.
///
/// # Arguments
/// * `skills_path` - Base directory of the skill library
/// * `curator_config` - Curator configuration (interval, thresholds, etc.)
/// * `llm` - The LLM client to use for the consolidation pass
///
/// # Returns
/// `Ok(Some(outcome))` if the pass ran, `Ok(None)` if `should_run()` returned
/// false, or an error string.
pub async fn run_curator_llm_if_needed(
    skills_path: &std::path::Path,
    curator_config: &CuratorConfig,
    base_config: ReactBuildConfig,
    force: bool,
) -> Result<Option<crate::curator_llm::CuratorLlmPassOutcome>, String> {
    let skills = SkillRegistry::new(skills_path);
    let state_path = skills_path.join("curator").join("state.json");
    let curator = Curator::new(SkillRegistry::new(skills_path), curator_config.clone())
        .with_state_path(state_path);

    if !force && !curator.should_run(None) {
        return Ok(None);
    }

    // Load usage reports for the prompt
    let usage_store = skill::SkillUsageStore::new(skills_path);
    let usage_reports = usage_store
        .agent_created_report()
        .unwrap_or_default();

    let outcome = crate::curator_llm::run_curator_llm_pass(base_config, &skills, &usage_store, &usage_reports)
        .await?;

    // Check for degraded run (Hermes "Never raises" — errors land in run_error)
    if let Some(ref err) = outcome.run_error {
        warn!("Curator LLM pass error: {}", err);
    }

    // Update curator state to record this run
    if let Err(e) = curator.mark_run_completed() {
        warn!("Curator: failed to mark run completed: {:?}", e);
    }

    // Persist per-run report (JSON + Markdown)
    let run_id = format!(
        "curator-{}",
        chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S")
    );
    let run_report = crate::curator::CuratorRunReport::from_llm_pass_outcome(&outcome, &run_id);
    let reports_dir = skills_path.join("curator").join("reports");
    match run_report.save_to_dir(&reports_dir) {
        Ok((json_path, md_path)) => {
            info!(
                "Curator report saved: {} + {}",
                json_path.display(),
                md_path.display()
            );
        }
        Err(e) => {
            warn!("Curator: failed to save run report: {:?}", e);
        }
    }

    info!(
        "Curator LLM pass completed: {} turns, {} tool calls, {} consolidated, {} pruned, {:.1}s",
        outcome.turns,
        outcome.all_tool_calls.len(),
        outcome.classification.consolidated.len(),
        outcome.classification.pruned.len(),
        outcome.elapsed_seconds,
    );

    Ok(Some(outcome))
}

pub fn skills_default_path_public() -> std::path::PathBuf {
    skills_default_path()
}

/// Never-raises wrapper around `run_curator_llm_if_needed`.
///
/// Swallows all errors and logs them as warnings. Designed for fire-and-forget
/// background curator runs where a failure should never propagate to the caller
/// or crash the host process.
///
/// Returns `None` on any error or when the curator decides not to run.
pub async fn maybe_run_curator(
    skills_path: &std::path::Path,
    curator_config: &CuratorConfig,
    base_config: ReactBuildConfig,
) -> Option<crate::curator_llm::CuratorLlmPassOutcome> {
    match run_curator_llm_if_needed(skills_path, curator_config, base_config, false).await {
        Ok(opt) => opt,
        Err(e) => {
            warn!("Curator background run failed (suppressed): {}", e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn registry_new_starts_empty_and_wait_all_returns_zero() {
        let registry = PendingReviewRegistry::new();
        assert_eq!(registry.wait_all().await, 0);
    }

#[tokio::test]
    async fn push_then_wait_all_returns_count_and_drains() {
        let registry = PendingReviewRegistry::new();
        let handle = tokio::spawn(async {});
        registry.push(handle);
        assert_eq!(registry.wait_all().await, 1);

        assert_eq!(registry.wait_all().await, 0);
    }

    #[tokio::test]
    async fn try_acquire_returns_guard_for_unseen_session() {
        let registry = Arc::new(PendingReviewRegistry::new());
        let guard = registry.try_acquire("session-a".into());
        assert!(guard.is_some());
        assert_eq!(registry.active_sessions(), 1);
    }

    #[tokio::test]
    async fn try_acquire_returns_none_when_session_already_in_flight() {
        let registry = Arc::new(PendingReviewRegistry::new());
        let _first = registry.try_acquire("session-a".into()).expect("first");
        let second = registry.try_acquire("session-a".into());
        assert!(second.is_none(), "duplicate acquire must be rejected");
        assert_eq!(registry.active_sessions(), 1);
    }

    #[tokio::test]
    async fn try_acquire_allows_distinct_sessions_concurrently() {
        let registry = Arc::new(PendingReviewRegistry::new());
        let a = registry.try_acquire("session-a".into()).expect("a");
        let b = registry.try_acquire("session-b".into()).expect("b");
        assert_eq!(registry.active_sessions(), 2);
        drop(a);
        drop(b);
        assert_eq!(registry.active_sessions(), 0);
    }

    #[tokio::test]
    async fn dropping_guard_releases_slot_for_reacquire() {
        let registry = Arc::new(PendingReviewRegistry::new());
        {
            let _guard = registry.try_acquire("session-a".into()).expect("first");
            assert_eq!(registry.active_sessions(), 1);
        }
        assert_eq!(registry.active_sessions(), 0);
        let _second = registry.try_acquire("session-a".into()).expect("after drop");
        assert_eq!(registry.active_sessions(), 1);
    }

    #[tokio::test]
    async fn wait_for_pending_reviews_invokes_global_registry() {
        let _ = wait_for_pending_reviews().await;
    }

    #[tokio::test]
    async fn wait_all_handles_completion_successfully() {
        let registry = PendingReviewRegistry::new();
        let handle = tokio::spawn(async {
            tokio::task::yield_now().await;
        });
        registry.push(handle);
        let count = registry.wait_all().await;
        assert_eq!(count, 1);
    }

    #[test]
    fn run_curator_if_needed_returns_ok_when_should_run_false() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_curator_if_needed(dir.path(), &CuratorConfig::default());
        assert!(result.is_ok());
    }

    #[test]
    fn skills_default_path_public_returns_a_path() {
        let p = skills_default_path_public();
        assert!(p.is_absolute() || p.components().count() > 0);
    }

    #[tokio::test]
    async fn run_curator_llm_if_needed_returns_none_when_not_forced_and_should_run_false() {
        let dir = tempfile::tempdir().unwrap();
        let result =
            run_curator_llm_if_needed(dir.path(), &CuratorConfig::default(), ReactBuildConfig::default(), false)
                .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn maybe_run_curator_returns_none_when_should_run_false() {
        let dir = tempfile::tempdir().unwrap();
        let result =
            maybe_run_curator(dir.path(), &CuratorConfig::default(), ReactBuildConfig::default()).await;
        assert!(result.is_none());
    }

    #[test]
    fn background_review_callbacks_default_is_empty() {
        let cb: BackgroundReviewCallbacks = Default::default();
        assert!(cb.on_output.is_none());
        assert!(cb.on_review_complete.is_none());
    }
}
