//! Models subcommand: list available models from configured providers.
//!
//! Queries the `/v1/models` endpoint of each configured provider and displays
//! the available models. Supports filtering by provider name.
//!
//! **Interaction**: Called from the `loom` binary when the user runs `loom models list`
//! or `loom models show <PROVIDER>`.

use config::{load_full_config, ProviderDef};
use loom::llm::{fetch_provider_models, ProviderModels};
use loom::RunError;

/// Maximum number of models to display per provider before truncating.
const MAX_MODELS_DISPLAY: usize = 30;

/// List models from all configured providers.
pub async fn list_all_models(json: bool) -> Result<(), RunError> {
    let config = load_full_config("loom").map_err(|e| {
        RunError::ConfigError(format!("Failed to load config: {}", e))
    })?;

    if config.providers.is_empty() {
        eprintln!("No providers configured in ~/.loom/config.toml");
        eprintln!("Add a [[providers]] section to your config file.");
        return Ok(());
    }

    let mut results: Vec<ProviderModels> = Vec::new();

    for provider in &config.providers {
        let result = query_provider_models(provider).await;
        results.push(result);
    }

    if json {
        output_json(&results);
    } else {
        output_human(&results);
    }

    Ok(())
}

/// List models from a specific provider.
pub async fn list_provider_models(provider_name: &str, json: bool) -> Result<(), RunError> {
    let config = load_full_config("loom").map_err(|e| {
        RunError::ConfigError(format!("Failed to load config: {}", e))
    })?;

    let provider = config
        .providers
        .iter()
        .find(|p| p.name == provider_name)
        .ok_or_else(|| RunError::ConfigError(format!("Provider '{}' not found", provider_name)))?;

    let result = query_provider_models(provider).await;

    if json {
        output_json(&[result]);
    } else {
        output_human(&[result]);
    }

    Ok(())
}

/// Query models from a provider.
async fn query_provider_models(provider: &ProviderDef) -> ProviderModels {
    let provider_type = provider.provider_type.as_deref().unwrap_or("openai");
    let base_url = provider.base_url.as_deref().unwrap_or("");
    let api_key = provider.api_key.as_deref().unwrap_or("");

    match fetch_provider_models(Some(provider_type), Some(base_url), Some(api_key)).await {
        Ok(models) => ProviderModels::ok(provider.name.clone(), models),
        Err(e) => ProviderModels::err(provider.name.clone(), e.to_string()),
    }
}

/// Output results as JSON.
fn output_json(results: &[ProviderModels]) {
    let json_results: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            let models: Vec<serde_json::Value> = r
                .models
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "created": m.created,
                        "owned_by": m.owned_by
                    })
                })
                .collect();
            serde_json::json!({
                "provider": r.provider,
                "models": models,
                "error": r.error
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&json_results).unwrap_or_default());
}

/// Output results in human-readable format.
fn output_human(results: &[ProviderModels]) {
    for result in results {
        println!("\n📦 Provider: {}", result.provider);
        println!("{}", "─".repeat(50));

        if let Some(ref error) = result.error {
            println!("  ❌ Error: {}", error);
            continue;
        }

        if result.models.is_empty() {
            println!("  No models available");
            continue;
        }

        let display_count = result.models.len().min(MAX_MODELS_DISPLAY);
        for model in result.models.iter().take(display_count) {
            println!("  • {}", model.id);
            if let Some(ref owned_by) = model.owned_by {
                println!("    Owner: {}", owned_by);
            }
        }

        if result.models.len() > MAX_MODELS_DISPLAY {
            println!(
                "  ... and {} more",
                result.models.len() - MAX_MODELS_DISPLAY
            );
        }
    }
}
