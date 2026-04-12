//! Models subcommand: list available models from model spec metadata.
//!
//! Reads configured providers from `~/.loom/config.toml`, resolves models from
//! model spec, and displays available models. Supports filtering by provider name.
//!
//! **Interaction**: Called from the `loom` binary when the user runs `loom models list`
//! or `loom models show <PROVIDER>`.

use config::{load_full_config, ProviderDef};
use loom::llm::{ModelInfo, ModelRegistry, ProviderConfig, ProviderModels};
use loom::RunError;
use std::collections::HashMap;

/// Maximum number of models to display per provider before truncating.
#[allow(dead_code)]
const MAX_MODELS_DISPLAY: usize = 30;

/// List models from all configured providers.
#[allow(dead_code)]
pub async fn list_all_models(json: bool) -> Result<(), RunError> {
    let config = load_full_config("loom")
        .map_err(|e| RunError::ConfigError(format!("Failed to load config: {}", e)))?;

    if config.providers.is_empty() {
        eprintln!("No providers configured in ~/.loom/config.toml");
        eprintln!("Add a [[providers]] section to your config file.");
        return Ok(());
    }

    let results = query_providers_models_from_spec(&config.providers).await;

    if json {
        output_json(&results);
    } else {
        output_human(&results);
    }

    Ok(())
}

/// List models from a specific provider.
#[allow(dead_code)]
pub async fn list_provider_models(provider_name: &str, json: bool) -> Result<(), RunError> {
    let config = load_full_config("loom")
        .map_err(|e| RunError::ConfigError(format!("Failed to load config: {}", e)))?;

    let provider = config
        .providers
        .iter()
        .find(|p| p.name == provider_name)
        .ok_or_else(|| RunError::ConfigError(format!("Provider '{}' not found", provider_name)))?;

    let result = query_providers_models_from_spec(std::slice::from_ref(provider))
        .await
        .into_iter()
        .next()
        .unwrap_or_else(|| ProviderModels::ok(provider.name.clone(), Vec::new()));

    if json {
        output_json(&[result]);
    } else {
        output_human(&[result]);
    }

    Ok(())
}

/// Query models from providers using model spec.
#[allow(dead_code)]
async fn query_providers_models_from_spec(providers: &[ProviderDef]) -> Vec<ProviderModels> {
    let registry = ModelRegistry::global();
    let provider_configs: Vec<ProviderConfig> = providers
        .iter()
        .map(|p| ProviderConfig {
            name: p.name.clone(),
            base_url: p.base_url.clone(),
            api_key: p.api_key.clone(),
            provider_type: p.provider_type.clone(),
            fetch_models: p.fetch_models.unwrap_or(false),
        })
        .collect();

    match registry.list_all_models_result(&provider_configs).await {
        Ok(entries) => {
            let mut by_provider: HashMap<String, Vec<ModelInfo>> = HashMap::new();
            for entry in entries {
                by_provider
                    .entry(entry.provider)
                    .or_default()
                    .push(ModelInfo {
                        id: entry.name,
                        created: None,
                        owned_by: None,
                    });
            }

            providers
                .iter()
                .map(|provider| {
                    let mut models = by_provider.remove(&provider.name).unwrap_or_default();
                    models.sort_by(|a, b| a.id.cmp(&b.id));
                    ProviderModels::ok(provider.name.clone(), models)
                })
                .collect()
        }
        Err(e) => providers
            .iter()
            .map(|provider| {
                ProviderModels::err(
                    provider.name.clone(),
                    format!("Failed to resolve models from model spec: {e}"),
                )
            })
            .collect(),
    }
}

/// Output results as JSON.
#[allow(dead_code)]
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

    println!(
        "{}",
        serde_json::to_string_pretty(&json_results).unwrap_or_default()
    );
}

/// Output results in human-readable format.
#[allow(dead_code)]
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
