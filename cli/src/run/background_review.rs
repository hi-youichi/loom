use crate::run::curator::{Curator, CuratorConfig};
use crate::run::evolution_trigger::{EvolutionTrigger, EvolutionTriggerConfig};
use crate::run::memory::MemoryStore;
use crate::run::observability::ObservabilityStore;
use crate::run::review_agent_loop::{
    build_review_agent_client, AgentReviewRunner, AgentReviewConfig, ReviewMode,
};
use crate::review_history::{ReviewHistory, ReviewRecord};
use crate::run::skill_registry::SkillRegistry;
use chrono::Utc;
use tracing::{error, info, warn};

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

pub async fn spawn_background_review(
    config: BackgroundReviewConfig,
    session_content: String,
    session_id: String,
) -> Result<(), String> {
    if !config.enabled {
        return Ok(());
    }

    if !config.review_memory && !config.review_skills {
        return Ok(());
    }

    if session_content.len() < config.min_session_chars {
        info!(
            "Skipping background review: session too short ({} chars)",
            session_content.len()
        );
        return Ok(());
    }

    let max_iterations = config.max_iterations;
    let max_session_chars = config.max_session_chars;
    let review_memory = config.review_memory;
    let review_skills = config.review_skills;
    let curator_config = config.curator_config;
    let curator_run_interval_secs = config.curator_run_interval_secs;
    let evolution_enabled = config.evolution_enabled;
    let evolution_config = config.evolution_config;
    let observability_enabled = config.observability_enabled;

    let llm = build_review_agent_client(&config.base_url, &config.api_key, &config.model);

    tokio::spawn(async move {
        let start = std::time::Instant::now();
        match run_background_review_inner(&*llm, &session_content, max_iterations, max_session_chars, review_memory, review_skills).await {
            Ok(result) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                info!("Background review completed: {} ({} actions, {}ms)", result.summary, result.action_count, duration_ms);

                let history = ReviewHistory::new(&config::home::loom_home());
                let record = ReviewRecord {
                    session_id: session_id.clone(),
                    reviewed_at: Utc::now(),
                    trigger: "auto".to_string(),
                    model: String::new(),
                    memory_update_count: result.memory_count,
                    skill_update_count: result.skill_count,
                    skipped: false,
                    skip_reason: None,
                    duration_ms,
                };
                if let Err(e) = history.append(&record) {
                    warn!("Failed to record background review: {}", e);
                }

                if result.action_count > 0 {
                    eprintln!("\n📚 Background review: {}", result.summary);
                }

                if observability_enabled {
                    let obs = ObservabilityStore::new(&ObservabilityStore::default_path());
                    obs.record_review(&session_id, result.memory_count, result.skill_count, duration_ms);
                }

                if let Err(e) = run_curator_if_needed(&SkillRegistry::default_path(), &curator_config, curator_run_interval_secs) {
                    warn!("Curator auto-run failed: {}", e);
                }

                if evolution_enabled {
                    if let Err(e) = run_evolution_if_eligible(&*llm, &evolution_config).await {
                        warn!("Evolution auto-run failed: {}", e);
                    }
                }
            }
            Err(e) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                error!("Background review failed: {} ({}ms)", e, duration_ms);

                let history = ReviewHistory::new(&config::home::loom_home());
                let record = ReviewRecord {
                    session_id: session_id.clone(),
                    reviewed_at: Utc::now(),
                    trigger: "auto".to_string(),
                    model: String::new(),
                    memory_update_count: 0,
                    skill_update_count: 0,
                    skipped: true,
                    skip_reason: Some(format!("llm_error: {}", e)),
                    duration_ms,
                };
                if let Err(he) = history.append(&record) {
                    warn!("Failed to record background review error: {}", he);
                }

                eprintln!("\n⚠️ Background review failed: {}", e);
            }
        }
    });

    Ok(())
}

struct BackgroundReviewHandle {
    pub summary: String,
    pub action_count: usize,
    pub memory_count: usize,
    pub skill_count: usize,
}

async fn run_background_review_inner(
    llm: &dyn loom::llm::LlmClient,
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

pub fn build_background_config_from_opts(opts: &loom::RunOptions) -> BackgroundReviewConfig {
    BackgroundReviewConfig {
        enabled: true,
        base_url: opts.base_url.clone().unwrap_or_default(),
        api_key: opts.api_key.clone().unwrap_or_default(),
        model: opts.model.clone().unwrap_or_else(|| "gpt-4o-mini".to_string()),
        ..Default::default()
    }
}

pub fn trigger_post_turn_review(opts: &loom::RunOptions, reply: &str) -> Result<(), String> {
    let config = build_background_config_from_opts(opts);
    let session_id = opts
        .thread_id
        .clone()
        .or_else(|| opts.session_id.clone())
        .unwrap_or_else(|| format!("auto-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()));

    let user_msg = match &opts.message {
        loom::UserContent::Text(t) => t.clone(),
        _ => String::new(),
    };

    let session_content = format!("User: {}\n\nAssistant: {}", user_msg, reply);

    let _ = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime for background review");
        rt.block_on(spawn_background_review(config, session_content, session_id))
    }).join()
    .map_err(|_| "Background review thread panicked".to_string())?;

    Ok(())
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

async fn run_evolution_if_eligible(
    llm: &dyn loom::llm::LlmClient,
    config: &EvolutionTriggerConfig,
) -> Result<(), String> {
    let trigger = EvolutionTrigger::new(llm, &SkillRegistry::default_path(), config.clone());
    let eligible = trigger.eligible_skills();

    if eligible.is_empty() {
        return Ok(());
    }

    info!("Evolution: {} skill(s) eligible for optimization", eligible.len());

    let obs = ObservabilityStore::new(&ObservabilityStore::default_path());

    for skill_name in &eligible {
        match trigger.try_evolve(skill_name).await {
            Ok(outcome) => {
                obs.record_evolution(
                    skill_name,
                    outcome.baseline_score,
                    outcome.best_score,
                    outcome.improved,
                );
                if outcome.improved {
                    info!(
                        "Evolved '{}' {:.3} -> {:.3}",
                        skill_name, outcome.baseline_score, outcome.best_score
                    );
                }
            }
            Err(e) => {
                warn!("Evolution failed for '{}': {}", skill_name, e);
            }
        }
    }

    Ok(())
}
