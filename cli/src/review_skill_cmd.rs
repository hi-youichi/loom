use crate::args::ReviewSkillArgs;
use cli::run::memory::MemoryStore;
use cli::run::review::{ReviewAgent, ReviewConfig, ReviewOutput};
use cli::run::review_agent_loop::ReviewMode;
use cli::run::skill_registry::SkillRegistry;
use config::load_full_config;
use loom_llm::{LlmClient, ModelEntry, ProviderConfig};
use loom_llm::client::RetryLlmClient;
use loom_tier::create_llm_client;
use std::io::{self, Read};
use std::sync::Arc;

pub(crate) fn build_review_client(
    model_override: Option<&str>,
) -> Result<Box<dyn LlmClient>, Box<dyn std::error::Error>> {
    let config = load_full_config("loom")?;

    let provider = if let Some(ref name) = config.default_provider {
        config
            .providers
            .iter()
            .find(|p| &p.name == name)
            .ok_or_else(|| format!("Default provider '{}' not found in config.toml", name))?
    } else {
        config
            .providers
            .first()
            .ok_or("No provider configured in config.toml")?
    };

    let model = model_override
        .map(|m| m.to_string())
        .or_else(|| std::env::var("LOOM_MODEL").ok())
        .or_else(|| std::env::var("MODEL").ok())
        .or_else(|| provider.model.clone())
        .unwrap_or_else(|| "gpt-4o-mini".to_string());

    let entry = ModelEntry::from_provider_config(
        &ProviderConfig {
            name: provider.name.clone(),
            base_url: provider.base_url.clone(),
            api_key: provider.api_key.clone(),
            provider_type: provider.provider_type.clone(),
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
        },
        &model,
    );

    let client = create_llm_client(&entry, None)?;

    let retry_client = RetryLlmClient::new(Arc::from(client))
        .with_max_retries(3)
        .with_base_delay(std::time::Duration::from_secs(2));

    Ok(Box::new(retry_client))
}

pub(crate) async fn handle_review_skill_command(
    args: &ReviewSkillArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let llm = build_review_client(args.model.as_deref())?;

    let model_name = args.model.as_deref().unwrap_or("(default)");
    eprintln!("Review Skill: using model {}", model_name);

    let input = if let Some(ref path) = args.input {
        std::fs::read_to_string(path)?
    } else {
        eprintln!("Reading from stdin (Ctrl+D to end)...");
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    };

    if input.trim().is_empty() {
        eprintln!("No input provided. Use --input <file> or pipe content via stdin.");
        std::process::exit(1);
    }

    eprintln!("Reviewing {} chars of input...", input.len());

    let memory = MemoryStore::new(&MemoryStore::default_path());
    let skills = SkillRegistry::new(&SkillRegistry::default_path());

    let review_config = ReviewConfig {
        auto_create_threshold: 1,
        max_session_chars: 24000,
        max_iterations: 10,
        mode: ReviewMode::Json,
    };

    let agent = ReviewAgent::with_config(llm, memory, skills, review_config);

    match agent.review_session(&input).await {
        Ok(output) => {
            print_review_results(&output);
        }
        Err(e) => {
            eprintln!("Review failed: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_review_results(output: &ReviewOutput) {
    eprintln!("\n━━━ Review Results ━━━");

    if output.memory_updates.is_empty() {
        eprintln!("  No memory updates.");
    } else {
        eprintln!("  Memory updates:");
        for update in &output.memory_updates {
            eprintln!(
                "    [{}] {} ({} chars)",
                update.action,
                update.file,
                update.content.len()
            );
            if update.content.len() <= 200 {
                eprintln!("      {}", update.content);
            } else {
                let truncated: String = update.content.chars().take(200).collect();
                eprintln!("      {}...", truncated);
            }
        }
    }

    if output.skill_suggestions.is_empty() {
        eprintln!("  No skill suggestions.");
    } else {
        eprintln!("  Skills created:");
        for skill in &output.skill_suggestions {
            eprintln!("    ✦ {} — {}", skill.name, skill.description);
            eprintln!("      Triggers: {:?}", skill.triggers);
            if skill.body.len() <= 200 {
                eprintln!("      Body: {}", skill.body);
            } else {
                let truncated: String = skill.body.chars().take(200).collect();
                eprintln!("      Body: {}...", truncated);
            }
        }
    }

    eprintln!("━━━━━━━━━━━━━━━━━━━━");
}
