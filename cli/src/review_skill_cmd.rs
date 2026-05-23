//! Triggers a background review of a session or file to extract skills
//! and memory updates. Uses the ReviewAgent with a real LLM.

use crate::args::ReviewSkillArgs;
use cli::run::memory::MemoryStore;
use cli::run::review::{ReviewAgent, ReviewConfig, ReviewLlm, ReviewOutput};
use cli::run::skill_registry::SkillRegistry;
use std::io::{self, Read};

/// Real LLM implementation using OpenAI-compatible API (blocking).
pub(crate) struct RealLlm {
    client: reqwest::blocking::Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl RealLlm {
    pub(crate) fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
            api_key,
            base_url,
            model,
        }
    }
}

impl ReviewLlm for RealLlm {
    fn complete(&self, prompt: &str) -> Result<String, String> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.3,
            "max_tokens": 4096
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            return Err(format!("API error {}: {}", status, text));
        }

        let json: serde_json::Value = response
            .json()
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        json.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "No content in API response".to_string())
    }
}

pub(crate) fn resolve_config() -> Result<(String, String, String), Box<dyn std::error::Error>> {
    // Get API key from env
    let api_key = std::env::var("OPENAI_API_KEY")
        .or_else(|_| std::env::var("LLM_API_KEY"))
        .map_err(|_| "No API key found. Set OPENAI_API_KEY or LLM_API_KEY environment variable.")?;

    // Get base URL
    let base_url = std::env::var("OPENAI_BASE_URL")
        .or_else(|_| std::env::var("LLM_BASE_URL"))
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

    // Get model (from args or env or default)
    let model = std::env::var("LOOM_MODEL")
        .or_else(|_| std::env::var("MODEL"))
        .unwrap_or_else(|_| "gpt-4o-mini".to_string());

    Ok((api_key, base_url, model))
}

pub(crate) async fn handle_review_skill_command(
    args: &ReviewSkillArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    run_review_skill(args)
}

fn run_review_skill(args: &ReviewSkillArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Resolve LLM config
    let (api_key, base_url, model) = resolve_config().map_err(|e| {
        format!(
            "Failed to resolve LLM config: {}\n\
             Hint: Set OPENAI_API_KEY and optionally OPENAI_BASE_URL, LOOM_MODEL",
            e
        )
    })?;

    let model = args.model.as_deref().unwrap_or(&model).to_string();

    eprintln!("Review Skill: using model {}", model);

    // Read input
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

    // Create LLM
    let llm = RealLlm::new(api_key, base_url, model);

    // Create memory store and skill registry
    let loom_home = config::home::loom_home();

    let memory = MemoryStore::new(&loom_home);
    let skills_dir = loom_home.join("skills");
    let skills = SkillRegistry::new(&skills_dir);

    // Create review agent with config
    let config = ReviewConfig {
        auto_create_threshold: 1, // Create immediately for manual command
        max_session_chars: 24000,
    };

    let agent = ReviewAgent::with_config(&llm, &memory, &skills, config);

    // Run review
    match agent.review_session(&input) {
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
            eprintln!("    [{}] {} ({} chars)", update.action, update.file, update.content.len());
            if update.content.len() <= 200 {
                eprintln!("      {}", update.content);
            } else {
                eprintln!("      {}...", &update.content[..200]);
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
                eprintln!("      Body: {}...", &skill.body[..200]);
            }
        }
    }

    eprintln!("━━━━━━━━━━━━━━━━━━━━");
}
