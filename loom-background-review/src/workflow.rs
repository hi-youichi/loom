//! Background review workflow - coordinates review agent and curator
//!
//! This module provides the main workflow for background review, including
//! spawning review tasks, configuring review agents, and running curator.
//!
//! Note: Some functionality depends on loom internals (LlmFactory, ModelEntry, etc.).
//! When used outside of loom, these features will need to be configured externally.

use super::curator::{Curator, CuratorConfig};

use super::memory::MemoryStore;
use super::observability::ObservabilityStore;
use super::agent_loop::{
    build_review_agent_client, AgentReviewRunner, AgentReviewConfig, ReviewMode,
};
use super::history::{ReviewHistory, ReviewRecord};
use super::skill_registry::SkillRegistry;
use chrono::Utc;
use std::sync::{Arc, Mutex as StdMutex};
use tracing::{error, info, warn};

/// Handle for the result of a background review.
pub struct BackgroundReviewHandle {
    pub summary: String,
    pub action_count: usize,
    pub memory_count: usize,
    pub skill_count: usize,
}

pub struct BackgroundReviewConfig {
    pub enabled: bool,
    pub max_session_chars: usize,
    pub max_iterations: u32,
    pub min_session_chars: usize,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub review_memory: bool,
    pub review_skills: bool,
    pub curator_config: CuratorConfig,
    pub observability_enabled: bool,
}

impl Default for BackgroundReviewConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_session_chars: 24000,
            max_iterations: 16,
            min_session_chars: 200,
            base_url: String::new(),
            api_key: String::new(),
            model: "gpt-4o-mini".to_string(),
            review_memory: true,
            review_skills: true,
            curator_config: CuratorConfig::default(),
            observability_enabled: true,
        }
    }
}

/// Global registry to track pending background review tasks.
static PENDING_REVIEWS: std::sync::OnceLock<Arc<PendingReviewRegistry>> = std::sync::OnceLock::new();

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

        for handle in handles {
            let _ = handle.await;
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

/// Spawn a background review task using the current Tokio runtime.
pub fn spawn_background_review(
    config: BackgroundReviewConfig,
    session_content: String,
    session_id: String,
    callbacks: BackgroundReviewCallbacks,
) {
    let registry = pending_reviews();
    let handle = tokio::spawn(async move {
        let start = std::time::Instant::now();
        let result = run_background_review_workflow(&config, &session_content, &session_id).await;
        match result {
            Ok((summary, action_count, _memory_count, _skill_count, duration_ms)) => {
                info!("Background review completed: {} ({} actions, {}ms)", summary, action_count, duration_ms);

                if action_count > 0 {
                    if let Some(ref on_output) = callbacks.on_output {
                        on_output(&summary);
                    }
                    if let Some(ref on_complete) = callbacks.on_review_complete {
                        on_complete(&format!("Self-improvement review: {}", summary));
                    }
                }

                if let Err(e) = run_curator_if_needed(&SkillRegistry::default_path(), &config.curator_config) {
                    warn!("Curator auto-run failed: {}", e);
                }
            }
            Err(e) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                error!("Background review failed: {} ({}ms)", e, duration_ms);
            }
        }
    });

    registry.push(handle);
}

/// Internal function that runs the background review workflow.
pub async fn run_background_review_workflow(
    config: &BackgroundReviewConfig,
    session_content: &str,
    session_id: &str,
) -> Result<(String, usize, usize, usize, u64), String> {
    if !config.enabled {
        return Ok(("disabled".to_string(), 0, 0, 0, 0));
    }

    if !config.review_memory && !config.review_skills {
        return Ok(("no review mode enabled".to_string(), 0, 0, 0, 0));
    }

    if session_content.len() < config.min_session_chars {
        info!(
            "Skipping background review: session too short ({} chars)",
            session_content.len()
        );
        return Ok(("session too short".to_string(), 0, 0, 0, 0));
    }

    if config.base_url.is_empty() || config.api_key.is_empty() {
        info!("Skipping background review: no API credentials configured");
        return Ok(("no credentials".to_string(), 0, 0, 0, 0));
    }

    let start = std::time::Instant::now();

    let llm = build_review_agent_client(&config.base_url, &config.api_key, &config.model);

    let result = run_background_review_inner(
        &*llm,
        session_content,
        config.max_iterations,
        config.max_session_chars,
        config.review_memory,
        config.review_skills,
    )
    .await?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let memory_count = result.memory_count;
    let skill_count = result.skill_count;

    let history = ReviewHistory::new(&env_config::home::loom_home());
    let record = ReviewRecord {
        session_id: session_id.to_string(),
        reviewed_at: Utc::now(),
        trigger: "auto".to_string(),
        model: String::new(),
        memory_update_count: memory_count,
        skill_update_count: skill_count,
        skipped: false,
        skip_reason: None,
        duration_ms,
    };
    if let Err(e) = history.append(&record) {
        warn!("Failed to record background review: {}", e);
    }

    if config.observability_enabled {
        let obs = ObservabilityStore::new(&ObservabilityStore::default_path());
        obs.record_review(session_id, memory_count, skill_count, duration_ms);
    }

    Ok((result.summary, result.action_count, memory_count, skill_count, duration_ms))
}

pub async fn run_background_review_inner(
    llm: &dyn loom_llm::LlmClient,
    session_content: &str,
    max_iterations: u32,
    max_session_chars: usize,
    review_memory: bool,
    review_skills: bool,
) -> Result<BackgroundReviewHandle, String> {
    let memory = MemoryStore::new(&MemoryStore::default_path());
    let skills = SkillRegistry::new(&SkillRegistry::default_path());

    let config = AgentReviewConfig {
        max_iterations,
        max_session_chars,
        mode: ReviewMode::Agent,
        review_memory,
        review_skills,
    };

    let result = AgentReviewRunner::run_with_refs(
        llm,
        &memory,
        &skills,
        session_content,
        &config,
    )
    .await?;

    let memory_count = result.actions.iter().filter(|a| a.kind == "memory").count();
    let skill_count = result.actions.iter().filter(|a| a.kind == "skill" || a.kind == "skill_file").count();

    Ok(BackgroundReviewHandle {
        summary: result.summary,
        action_count: result.actions.len(),
        memory_count,
        skill_count,
    })
}

pub async fn wait_for_pending_reviews() -> usize {
    pending_reviews().wait_all().await
}

fn run_curator_if_needed(
    skills_path: &std::path::Path,
    curator_config: &CuratorConfig,
) -> Result<(), String> {
    let skills = SkillRegistry::new(skills_path);
    let state_path = skills_path.join("curator").join("state.json");
    let curator = Curator::new(skills, curator_config.clone()).with_state_path(state_path);


    // Phase 2.2: Use Curator four-gate system
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

/// Build background review config from external options.
///
/// This function is provided for external crates that need to build
/// a BackgroundReviewConfig from their own options structure.
/// When used outside of loom, you'll need to construct the config manually.
pub fn build_background_config_from_opts_ext(
    base_url: String,
    api_key: String,
    model: String,
    enabled: bool,
) -> BackgroundReviewConfig {
    BackgroundReviewConfig {
        enabled,
        base_url,
        api_key,
        model,
        ..Default::default()
    }
}

#[cfg(feature = "testing")]
pub fn build_background_config_from_opts_test(
    base_url: String,
    api_key: String,
    model: String,
) -> BackgroundReviewConfig {
    BackgroundReviewConfig {
        base_url,
        api_key,
        model,
        ..Default::default()
    }
}
