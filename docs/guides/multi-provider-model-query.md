# Multi-Provider Model Query Design

## Overview

This document describes the design for querying available models from multiple LLM providers in Loom. The solution extends the existing `LlmClient` trait to support dynamic model listing via the standard `/v1/models` endpoint.

## Architecture

### 1. Core Data Structures

```rust
/// Model information returned by provider's /v1/models endpoint
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub created: Option<i64>,
    pub owned_by: Option<String>,
}

/// Capability flags for a model
#[derive(Debug, Clone, Default)]
pub struct ModelCapabilities {
    pub chat_completions: bool,
    pub streaming: bool,
    pub tools: bool,
    pub vision: bool,
}
```

### 2. Extended LlmClient Trait

Add to `loom/src/llm/mod.rs`:

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    // ... existing methods ...
    
    /// List available models from the provider's /v1/models endpoint
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError>;
    
    /// Get model capabilities (provider-specific inference)
    fn get_model_capabilities(&self, model_id: &str) -> ModelCapabilities;
}
```

### 3. Provider Implementations

#### OpenAI (`loom/src/llm/openai.rs`)

```rust
#[async_trait]
impl LlmClient for ChatOpenAI {
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let url = format!("{}/models", self.config.base_url.trim_end_matches("/v1"));
        
        let res = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        if !res.status().is_success() {
            return Err(LlmError::ApiError(format!(
                "List models failed: {}",
                res.status()
            )));
        }

        let data: serde_json::Value = res
            .json()
            .await
            .map_err(|e| LlmError::ParseError(e.to_string()))?;

        let models = data["data"]
            .as_array()
            .ok_or_else(|| LlmError::ParseError("Missing 'data' field".into()))?
            .iter()
            .filter_map(|m| {
                Some(ModelInfo {
                    id: m["id"].as_str()?.to_string(),
                    created: m["created"].as_i64(),
                    owned_by: m["owned_by"].as_str().map(String::from),
                })
            })
            .collect();

        Ok(models)
    }

    fn get_model_capabilities(&self, model_id: &str) -> ModelCapabilities {
        let mut caps = ModelCapabilities::default();
        
        if model_id.contains("gpt-4") {
            caps.chat_completions = true;
            caps.streaming = true;
            caps.tools = true;
            caps.vision = model_id.contains("vision") || model_id.contains("gpt-4o");
        } else if model_id.contains("gpt-3.5") {
            caps.chat_completions = true;
            caps.streaming = true;
            caps.tools = true;
        }
        
        caps
    }
}
```

#### BigModel (`loom/src/llm/bigmodel.rs`)

```rust
#[async_trait]
impl LlmClient for ChatBigModel {
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let url = format!("{}/models", self.config.base_url.trim_end_matches("/v1"));
        
        let res = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        let data: serde_json::Value = res
            .json()
            .await
            .map_err(|e| LlmError::ParseError(e.to_string()))?;

        let models = data["data"]
            .as_array()
            .ok_or_else(|| LlmError::ParseError("Missing 'data' field".into()))?
            .iter()
            .filter_map(|m| {
                Some(ModelInfo {
                    id: m["id"].as_str()?.to_string(),
                    created: m["created"].as_i64(),
                    owned_by: m["owned_by"].as_str().map(String::from),
                })
            })
            .collect();

        Ok(models)
    }

    fn get_model_capabilities(&self, model_id: &str) -> ModelCapabilities {
        let mut caps = ModelCapabilities::default();
        
        // BigModel capability detection
        if model_id.starts_with("glm-") {
            caps.chat_completions = true;
            caps.streaming = true;
            caps.tools = model_id.contains("glm-4");
            caps.vision = model_id.contains("glm-4v") || model_id.contains("vision");
        }
        
        caps
    }
}
```

### 4. Model Cache Service

Create `config/src/model_fetcher.rs`:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Caches model lists per provider to avoid repeated API calls
pub struct ModelCache {
    cache: Arc<RwLock<HashMap<String, (Vec<ModelInfo>, Instant)>>>,
    ttl: Duration,
}

impl ModelCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }

    pub async fn get_or_fetch<F, Fut>(
        &self,
        provider_name: &str,
        fetch_fn: F,
    ) -> Result<Vec<ModelInfo>, LlmError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<ModelInfo>, LlmError>>,
    {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some((models, fetched_at)) = cache.get(provider_name) {
                if fetched_at.elapsed() < self.ttl {
                    return Ok(models.clone());
                }
            }
        }

        // Fetch fresh data
        let models = fetch_fn().await?;
        
        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(provider_name.to_string(), (models.clone(), Instant::now()));
        }

        Ok(models)
    }
}

/// Query all providers and return aggregated model list
pub async fn list_all_provider_models(
    providers: &[ProviderDef],
    cache: &ModelCache,
) -> HashMap<String, Vec<ModelInfo>> {
    use loom::llm::{ChatBigModel, ChatOpenAI, BigModelConfig, OpenAIConfig};
    
    let mut results = HashMap::new();
    
    for provider in providers {
        let models = match provider.provider_type.as_deref() {
            Some("openai") | None => {
                let client = ChatOpenAI::with_config(OpenAIConfig {
                    api_key: provider.api_key.clone().unwrap_or_default(),
                    base_url: provider.base_url.clone().unwrap_or_default(),
                    ..Default::default()
                });
                cache.get_or_fetch(&provider.name, || async {
                    client.list_models().await
                }).await.ok()
            }
            Some("bigmodel") => {
                let client = ChatBigModel::with_config(BigModelConfig {
                    api_key: provider.api_key.clone().unwrap_or_default(),
                    base_url: provider.base_url.clone().unwrap_or_default(),
                    ..Default::default()
                });
                cache.get_or_fetch(&provider.name, || async {
                    client.list_models().await
                }).await.ok()
            }
            _ => None,
        };
        
        if let Some(models) = models {
            results.insert(provider.name.clone(), models);
        }
    }
    
    results
}
```

### 5. Configuration Example

```toml
# ~/.loom/config.toml

[env]
# Default provider settings
OPENAI_API_KEY = "sk-..."
OPENAI_BASE_URL = "https://api.openai.com/v1"
MODEL = "gpt-4o"

[[providers]]
name = "openai"
api_key = "sk-..."
base_url = "https://api.openai.com/v1"
type = "openai"
model = "gpt-4o"

[[providers]]
name = "bigmodel"
api_key = "xxx.yyy"
base_url = "https://open.bigmodel.cn/api/paas/v4"
type = "bigmodel"
model = "glm-4-flash"

[[providers]]
name = "local-ollama"
base_url = "http://localhost:11434/v1"
type = "openai"  # Ollama is OpenAI-compatible
model = "llama3.1"

[default]
provider = "openai"
```

### 6. CLI Integration

Add to `cli/src/main.rs` or `cli/src/tool_cmd.rs`:

```rust
/// List available models from all configured providers
async fn cmd_list_models(config: &FullConfig) -> Result<(), Box<dyn Error>> {
    let cache = ModelCache::new(Duration::from_secs(300)); // 5-minute TTL
    
    println!("Fetching available models from providers...\n");
    
    let all_models = list_all_provider_models(&config.providers, &cache).await;
    
    for (provider_name, models) in all_models {
        println!("📦 Provider: {}", provider_name);
        println!("{}", "─".repeat(50));
        
        for model in models.iter().take(20) {
            println!("  • {}", model.id);
            if let Some(owned_by) = &model.owned_by {
                println!("    Owner: {}", owned_by);
            }
        }
        
        if models.len() > 20 {
            println!("  ... and {} more", models.len() - 20);
        }
        println!();
    }
    
    Ok(())
}

/// Validate that configured models are available
async fn cmd_validate_models(config: &FullConfig) -> Result<(), Box<dyn Error>> {
    let cache = ModelCache::new(Duration::from_secs(60));
    let all_models = list_all_provider_models(&config.providers, &cache).await;
    
    let mut all_valid = true;
    
    for provider in &config.providers {
        if let Some(models) = all_models.get(&provider.name) {
            let model_ids: Vec<_> = models.iter().map(|m| &m.id).collect();
            
            if let Some(configured_model) = &provider.model {
                if model_ids.contains(&configured_model) {
                    println!("✅ {}: model '{}' is available", provider.name, configured_model);
                } else {
                    println!("❌ {}: model '{}' NOT FOUND in available models", provider.name, configured_model);
                    all_valid = false;
                }
            }
        } else {
            println!("⚠️  {}: Could not fetch model list", provider.name);
        }
    }
    
    if all_valid {
        println!("\n✨ All configured models are valid!");
    } else {
        println!("\n⚠️  Some configured models are not available");
    }
    
    Ok(())
}
```

## Usage Examples

### Command Line

```bash
# List all available models from all providers
loom models list

# List models from specific provider
loom models list --provider bigmodel

# Search for specific models
loom models search "gpt-4"

# Validate configured models
loom models validate

# Show model capabilities
loom models info gpt-4o
```

### Programmatic API

```rust
use config::{load_full_config, model_fetcher::{ModelCache, list_all_provider_models}};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = load_full_config("loom")?;
    
    // Create cache with 5-minute TTL
    let cache = ModelCache::new(Duration::from_secs(300));
    
    // Query all providers
    let models = list_all_provider_models(&config.providers, &cache).await;
    
    for (provider, model_list) in models {
        println!("Provider '{}' has {} models", provider, model_list.len());
        for model in model_list {
            println!("  - {}", model.id);
        }
    }
    
    Ok(())
}
```

## Implementation Checklist

- [ ] Add `ModelInfo` and `ModelCapabilities` structs to `loom/src/llm/mod.rs`
- [ ] Extend `LlmClient` trait with `list_models()` and `get_model_capabilities()`
- [ ] Implement `list_models()` for `ChatOpenAI`
- [ ] Implement `list_models()` for `ChatBigModel`
- [ ] Create `config/src/model_fetcher.rs` with `ModelCache`
- [ ] Add CLI commands: `models list`, `models search`, `models validate`
- [ ] Add tests for model fetching and caching
- [ ] Update documentation

## Key Design Decisions

| Aspect | Decision | Rationale |
|--------|----------|-----------|
| **Endpoint** | `/v1/models` | Standard OpenAI-compatible endpoint |
| **Caching** | 5-minute TTL in memory | Reduces API calls while staying fresh |
| **Capability Detection** | Pattern-based inference | Avoids extra API calls, extensible |
| **Error Handling** | Graceful degradation | Failed provider queries don't block others |
| **Base URL Reuse** | Trim `/v1` suffix | Works with both `/v1` and root URLs |

## Future Enhancements

1. **Persistent Cache**: Store model lists in SQLite for offline access
2. **Model Metadata**: Add context window size, pricing, rate limits
3. **Auto-Selection**: Recommend models based on task requirements
4. **Provider Health**: Track provider availability and latency
5. **Model Aliases**: Map user-friendly names to provider-specific IDs
