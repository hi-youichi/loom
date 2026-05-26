use super::curator::{Curator, CuratorConfig};
use super::evolution::EvolutionTriggerConfig;
use super::memory::MemoryStore;
use super::observability::ObservabilityStore;
use super::agent_loop::{
    build_review_agent_client, AgentReviewRunner, AgentReviewConfig, ReviewMode,
};
use super::history::{ReviewHistory, ReviewRecord};
use super::skill_registry::SkillRegistry;
use crate::llm::{LlmFactory, ModelEntry};
use chrono::Utc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    pub session_model: Option<ModelEntry>,
    pub review_memory: bool,
    pub review_skills: bool,
    pub curator_config: CuratorConfig,
    pub curator_run_interval_secs: u64,
    pub evolution_enabled: bool,
    pub evolution_config: EvolutionTriggerConfig,
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
            session_model: None,
            review_memory: true,
            review_skills: true,
            curator_config: CuratorConfig::default(),
            curator_run_interval_secs: 86400,
            evolution_enabled: true,
            evolution_config: EvolutionTriggerConfig::default(),
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
    counter: AtomicUsize,
    handles: StdMutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl PendingReviewRegistry {
    fn new() -> Self {
        Self {
            counter: AtomicUsize::new(0),
            handles: StdMutex::new(Vec::new()),
        }
    }

    pub fn push(&self, handle: tokio::task::JoinHandle<()>) {
        let _id = self.counter.fetch_add(1, Ordering::Relaxed);
        let mut handles = self.handles.lock().unwrap();
        handles.push(handle);
    }

    pub async fn wait_all(&self) -> usize {
        let mut handles = self.handles.lock().unwrap();
        let count = handles.len();
        if count == 0 {
            return 0;
        }

        info!("Waiting for {} background review(s) to complete...", count);

        for handle in handles.drain(..) {
            let _ = handle.await;
        }

        info!("All {} background review(s) completed.", count);
        count
    }
}

/// Callback type for review output notifications.
pub type ReviewOutputFn = Arc<dyn Fn(&str) + Send + Sync>;

/// Spawn a background review task using the current Tokio runtime.
pub fn spawn_background_review(
    config: BackgroundReviewConfig,
    session_content: String,
    session_id: String,
    on_output: Option<ReviewOutputFn>,
) {
    let registry = pending_reviews();
    let handle = tokio::spawn(async move {
        let start = std::time::Instant::now();
        let result = run_background_review_workflow(&config, &session_content, &session_id).await;
        match result {
            Ok((summary, action_count, _memory_count, _skill_count, duration_ms)) => {
                info!("Background review completed: {} ({} actions, {}ms)", summary, action_count, duration_ms);

                if action_count > 0 {
                    if let Some(ref on_output) = on_output {
                        on_output(&summary);
                    }
                }

                if let Err(e) = run_curator_if_needed(&SkillRegistry::default_path(), &config.curator_config, config.curator_run_interval_secs) {
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
async fn run_background_review_workflow(
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

    let (review_base_url, review_api_key, review_model) = resolve_review_model(config).await;
    if review_base_url.is_empty() || review_api_key.is_empty() {
        info!("Skipping background review: resolved credentials are empty");
        return Ok(("no credentials".to_string(), 0, 0, 0, 0));
    }
    let llm = build_review_agent_client(&review_base_url, &review_api_key, &review_model);

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

async fn run_background_review_inner(
    llm: &dyn crate::llm::LlmClient,
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

async fn resolve_review_model(config: &BackgroundReviewConfig) -> (String, String, String) {
    if let Some(ref session_entry) = config.session_model {
        if let Some(factory) = LlmFactory::load() {
            if let Some(strong_entry) = factory
                .resolve_tier_from_entry(session_entry, crate::model_spec::ModelTier::Strong)
                .await
            {
                return (
                    strong_entry.base_url.unwrap_or_else(|| config.base_url.clone()),
                    strong_entry.api_key.unwrap_or_else(|| config.api_key.clone()),
                    strong_entry.name.clone(),
                );
            }
        }
    }
    (config.base_url.clone(), config.api_key.clone(), config.model.clone())
}

pub fn build_background_config_from_opts(opts: &crate::RunOptions) -> BackgroundReviewConfig {
    let session_model = resolve_session_model(opts);

    let base_url = opts.base_url.clone()
        .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
        .unwrap_or_default();
    let api_key = opts.api_key.clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .unwrap_or_default();

    let model = opts.model.clone()
        .or_else(|| std::env::var("MODEL").ok())
        .unwrap_or_else(|| "gpt-4o-mini".to_string());

    BackgroundReviewConfig {
        enabled: true,
        base_url,
        api_key,
        model,
        session_model,
        ..Default::default()
    }
}

fn resolve_session_model(opts: &crate::RunOptions) -> Option<ModelEntry> {
    let model = opts.model.as_deref()?;
    let (provider, _) = crate::llm::ModelEntry::parse_id(model)?;
    let providers = crate::provider::load_provider_configs()?;
    let entry = crate::tier::resolve::resolve_from_plan(
        provider,
        crate::model_spec::ModelTier::Standard,
        &providers,
    )?;
    let mut entry = entry;
    entry.base_url = entry.base_url.or_else(|| opts.base_url.clone());
    entry.api_key = entry.api_key.or_else(|| opts.api_key.clone());
    Some(entry)
}

pub async fn wait_for_pending_reviews() -> usize {
    pending_reviews().wait_all().await
}

fn run_curator_if_needed(
    skills_path: &std::path::Path,
    curator_config: &CuratorConfig,
    interval_secs: u64,
) -> Result<(), String> {
    let state_path = skills_path.join("curator").join("state.json");
    let last_run = state_path.metadata().ok().and_then(|m| m.modified().ok())
        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs());

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if let Some(last) = last_run {
        if now_secs.saturating_sub(last) < interval_secs {
            return Ok(());
        }
    }

    let skills = SkillRegistry::new(skills_path);
    let curator = Curator::new(skills, curator_config.clone())
        .with_state_path(state_path);

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

