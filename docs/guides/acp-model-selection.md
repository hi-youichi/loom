# ACP Model Selection Implementation Plan

This document describes the implementation plan for supporting model selection in the ACP (Agent Client Protocol) integration.

## Current State

The ACP integration already has basic model configuration support:

- `loom-acp/src/agent.rs:530-545` defines `build_model_config_options` function with a `model` config option
- The `options` array is currently empty (`"options": []`)
- `loom::llm::fetch_provider_models` can fetch available models from providers
- `config::load_full_config` loads configured providers from `~/.loom/config.toml`

## Architecture

```
┌─────────────────┐
│   ACP Client    │ (IDE: Zed, JetBrains, etc.)
│                 │
└────────┬────────┘
         │ JSON-RPC (stdio)
         ▼
┌─────────────────┐     ┌──────────────────┐
│  LoomAcpAgent   │────▶│  config module   │
│                 │     │  (providers)     │
└────────┬────────┘     └──────────────────┘
         │                       │
         ▼                       ▼
┌─────────────────┐     ┌──────────────────┐
│  build_model_   │     │  loom::llm       │
│  config_options │◀────│  (fetch_models)  │
└─────────────────┘     └──────────────────┘
```

## Implementation

### 1. Model Option Structure

Add a structure to represent a model option in the select dropdown:

```rust
// loom-acp/src/agent.rs

#[derive(Debug, Clone, Serialize)]
struct ModelOption {
    /// Model identifier (e.g., "gpt-4o", "claude-3-opus")
    id: String,
    /// Display name for the model
    name: String,
    /// Provider name (e.g., "openai", "anthropic")
    provider: String,
}
```

### 2. Fetch Available Models

Add a method to fetch models from all configured providers:

```rust
// loom-acp/src/agent.rs

use config::load_full_config;
use loom::llm::fetch_provider_models;

impl LoomAcpAgent {
    /// Fetch available models from all configured providers.
    async fn get_available_models(&self) -> Vec<ModelOption> {
        let config = match load_full_config("loom") {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        
        let mut all_models = vec![];
        
        for (name, provider) in &config.providers {
            match fetch_provider_models(
                Some(provider.r#type.clone()),
                Some(provider.base_url.clone()),
                Some(provider.api_key.clone()),
            ).await {
                Ok(models) => {
                    for model in models.models {
                        all_models.push(ModelOption {
                            id: model.id.clone(),
                            name: model.id,
                            provider: name.clone(),
                        });
                    }
                }
                Err(_) => continue,
            }
        }
        
        all_models
    }
}
```

### 3. Update `build_model_config_options`

Modify the function to accept and include model options:

```rust
// loom-acp/src/agent.rs

fn build_model_config_options(
    current_model: &str,
    model_options: Vec<ModelOption>,
) -> Result<Vec<agent_client_protocol::SessionConfigOption>, serde_json::Error> {
    let options: Vec<_> = model_options
        .iter()
        .map(|m| serde_json::json!({
            "id": &m.id,
            "name": &m.name,
        }))
        .collect();
    
    let json = serde_json::json!([
        {
            "id": "model",
            "name": "Model",
            "description": "LLM model for this session.",
            "category": "model",
            "type": "select",
            "currentValue": current_model,
            "options": options
        }
    ]);
    serde_json::from_value(json)
}
```

### 4. Update `new_session`

Call the new method to populate model options:

```rust
// loom-acp/src/agent.rs

async fn new_session(
    &self,
    args: NewSessionRequest,
) -> agent_client_protocol::Result<NewSessionResponse> {
    crate::logging::init_with_working_folder(&args.cwd);

    let working_directory = Some(args.cwd.clone());
    let our_id = self.sessions.create(working_directory);
    let session_id = SessionId::new(our_id.as_str().to_string());
    
    let current_model = std::env::var("MODEL")
        .unwrap_or_else(|_| std::env::var("OPENAI_MODEL").unwrap_or_default());
    
    // Fetch available models from providers
    let model_options = self.get_available_models().await;
    
    let config_options = build_model_config_options(&current_model, model_options)
        .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;
    
    Ok(NewSessionResponse::new(session_id).config_options(Some(config_options)))
}
```

### 5. Optional: Add Caching

To avoid querying provider APIs on every `new_session`, add a cache:

```rust
// loom-acp/src/agent.rs

use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{Duration, Instant};

pub struct LoomAcpAgent {
    checkpointer: Arc<SqliteSaver>,
    sessions: SessionStore,
    /// Cached model list with expiration time
    model_cache: Arc<RwLock<Option<(Vec<ModelOption>, Instant)>>>,
}

impl LoomAcpAgent {
    const CACHE_TTL: Duration = Duration::from_secs(300); // 5 minutes
    
    pub fn new(checkpointer: Arc<SqliteSaver>) -> Self {
        Self {
            checkpointer,
            sessions: SessionStore::new(),
            model_cache: Arc::new(RwLock::new(None)),
        }
    }
    
    async fn get_available_models(&self) -> Vec<ModelOption> {
        // Check cache
        {
            let cache = self.model_cache.read().await;
            if let Some((models, timestamp)) = cache.as_ref() {
                if timestamp.elapsed() < Self::CACHE_TTL {
                    return models.clone();
                }
            }
        }
        
        // Fetch new data
        let models = self.fetch_models_from_providers().await;
        
        // Update cache
        {
            let mut cache = self.model_cache.write().await;
            *cache = Some((models.clone(), Instant::now()));
        }
        
        models
    }
    
    async fn fetch_models_from_providers(&self) -> Vec<ModelOption> {
        // Implementation from step 2
    }
}
```

## Implementation Steps

| Step | File | Description |
|------|------|-------------|
| 1 | `loom-acp/src/agent.rs` | Add `ModelOption` struct |
| 2 | `loom-acp/src/agent.rs` | Add `get_available_models` method |
| 3 | `loom-acp/src/agent.rs` | Update `build_model_config_options` signature and implementation |
| 4 | `loom-acp/src/agent.rs` | Update `new_session` to call `get_available_models` |
| 5 | `loom-acp/src/agent.rs` | (Optional) Add `model_cache` field and caching logic |

## Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_model_config_options_includes_available_models() {
        let agent = LoomAcpAgent::new(Arc::new(SqliteSaver::new_in_memory().unwrap()));
        let response = agent.new_session(NewSessionRequest::default()).await.unwrap();
        
        let config_options = response.config_options.unwrap();
        let model_option = &config_options[0];
        
        // Verify options are populated (requires configured providers)
        // assert!(!model_option.options.is_empty());
    }
    
    #[test]
    fn test_build_model_config_options() {
        let models = vec![
            ModelOption { id: "gpt-4o".into(), name: "GPT-4o".into(), provider: "openai".into() },
            ModelOption { id: "gpt-4o-mini".into(), name: "GPT-4o Mini".into(), provider: "openai".into() },
        ];
        
        let result = build_model_config_options("gpt-4o", models).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id.as_str(), "model");
    }
}
```

## Dependencies

- `loom::llm::fetch_provider_models` - Fetch models from provider's `/v1/models` endpoint
- `config::load_full_config` - Load providers from `~/.loom/config.toml`
- `agent_client_protocol::SessionConfigOption` - ACP protocol type for config options

## Related Documentation

- [ACP Guide](./acp.md) - General ACP integration documentation
- [LLM Integration](./llm-integration.md) - LLM provider configuration
- [Configuration](./configuration.md) - Config file format
