//! Background review workflow infrastructure
//!
//! Provides the global `PendingReviewRegistry` and Curator auto-run helper.
//! The actual review execution lives in `crate::review`.

use super::curator::{Curator, CuratorConfig};
use super::skill_registry::{default_path as skills_default_path, SkillRegistry};
use loom_react_config::ReactBuildConfig;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tracing::{info, warn};

/// Global registry to track pending background review tasks.
static PENDING_REVIEWS: std::sync::OnceLock<Arc<PendingReviewRegistry>> =
    std::sync::OnceLock::new();

fn pending_reviews() -> &'static Arc<PendingReviewRegistry> {
    PENDING_REVIEWS.get_or_init(|| Arc::new(PendingReviewRegistry::new()))
}

/// Registry of pending background review handles.
pub struct PendingReviewRegistry {
    handles: StdMutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl PendingReviewRegistry {
    fn new() -> Self {
        Self {
            handles: StdMutex::new(Vec::new()),
        }
    }

    pub fn push(&self, handle: tokio::task::JoinHandle<()>) {
        let mut handles = self.handles.lock().unwrap();
        handles.push(handle);
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
