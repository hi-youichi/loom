//! Unified model registry for all providers.
//!
//! Provides a single source of truth for all available models across providers.
//! Combines provider configuration with model lists to return fully resolved model entries.
//!
//! # Example
//!
//! ```ignore
//! use loom::llm::{ModelRegistry, ProviderConfig};
//!
//! let registry = ModelRegistry::global();
//!
//! // List all models from all providers
//! let providers = vec![ProviderConfig {
//!     name: "openai".to_string(),
//!     base_url: Some("https://api.openai.com/v1".to_string()),
//!     api_key: Some("sk-...".to_string()),
//!     provider_type: None,
//! }];
//! let models = registry.list_all_models(&providers).await;
//!
//! // Get specific model with full config
//! let model = registry.get_model("openai/gpt-4o", &providers).await;
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use crate::error::AgentError;
use crate::llm::{fetch_provider_models, ChatBigModel, ChatOpenAI, LlmClient, ModelInfo};
use async_openai::config::OpenAIConfig;

/// Default TTL for cached model lists (5 minutes).
const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(300);

/// Provider configuration for model registry.
/// This is a simplified version that can be converted from config::ProviderDef.
#[derive(Clone, Debug)]
pub struct ProviderConfig {
    /// Unique provider name.
    pub name: String,
    /// Base URL of the API endpoint.
    pub base_url: Option<String>,
    /// API key for authentication.
    pub api_key: Option<String>,
    /// Provider type: "openai" (default) or "bigmodel".
    pub provider_type: Option<String>,
}

/// A fully resolved model entry with provider configuration.
///
/// This is the single source of truth for model configuration in the system.
/// It contains all information needed to create an LLM client and run the agent.
#[derive(Clone, Debug, Default)]
pub struct ModelEntry {
    // === Identity ===
    /// Unique identifier in format "{provider}/{model_id}" (e.g., "openai/gpt-4o").
    pub id: String,
    /// Display name (just the model id, e.g., "gpt-4o").
    pub name: String,
    /// Provider name.
    pub provider: String,

    // === Provider Configuration ===
    /// Base URL from provider config.
    pub base_url: Option<String>,
    /// API key from provider config.
    pub api_key: Option<String>,
    /// Provider type from provider config (e.g., "openai", "bigmodel").
    pub provider_type: Option<String>,

    // === Runtime Configuration ===
    /// Sampling temperature (0.0 - 2.0).
    pub temperature: Option<f32>,
    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,
    /// Tool choice mode (auto, none, required).
    pub tool_choice: Option<crate::llm::ToolChoiceMode>,
}

impl ModelEntry {
    /// Create a new ModelEntry with minimal required fields.
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        let provider = provider.into();
        let model = model.into();
        Self {
            id: format!("{}/{}", provider, model),
            name: model,
            provider,
            ..Default::default()
        }
    }

    /// Set the base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Set the API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Set the provider type.
    pub fn with_provider_type(mut self, provider_type: impl Into<String>) -> Self {
        self.provider_type = Some(provider_type.into());
        self
    }

    /// Set the temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Set the max tokens.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set the tool choice mode.
    pub fn with_tool_choice(mut self, tool_choice: crate::llm::ToolChoiceMode) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    /// Parse a model ID in format "provider/model" and return (provider, model).
    pub fn parse_id(id: &str) -> Option<(&str, &str)> {
        id.split_once('/')
    }

    /// Check if this entry has all required configuration to create an LLM client.
    pub fn is_complete(&self) -> bool {
        !self.name.is_empty() && self.api_key.is_some()
    }

    /// Create an LLM client from this entry.
    pub fn create_client(&self) -> Result<Box<dyn LlmClient>, AgentError> {
        create_llm_client(self)
    }

    /// Create a ModelEntry from a ProviderConfig and model name.
    pub fn from_provider_config(provider: &ProviderConfig, model: &str) -> Self {
        Self {
            id: format!("{}/{}", provider.name, model),
            name: model.to_string(),
            provider: provider.name.clone(),
            base_url: provider.base_url.clone(),
            api_key: provider.api_key.clone(),
            provider_type: provider.provider_type.clone(),
            ..Default::default()
        }
    }
}

/// Cached model list for a single provider.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct CachedProviderModels {
    /// Provider name.
    provider_name: String,
    /// List of models.
    models: Vec<ModelInfo>,
    /// When the models were fetched.
    fetched_at: Instant,
    /// Provider configuration.
    base_url: Option<String>,
    api_key: Option<String>,
    provider_type: Option<String>,
}

impl CachedProviderModels {
    fn is_expired(&self, ttl: Duration) -> bool {
        self.fetched_at.elapsed() > ttl
    }
}

/// Global model registry that caches model lists from all providers.
#[derive(Clone)]
pub struct ModelRegistry {
    inner: Arc<RwLock<RegistryInner>>,
    ttl: Duration,
}

#[derive(Default)]
struct RegistryInner {
    /// Cached models per provider.
    cache: Vec<CachedProviderModels>,
}

impl ModelRegistry {
    /// Create a new ModelRegistry with default TTL (5 minutes).
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_CACHE_TTL)
    }

    /// Create a new ModelRegistry with custom TTL.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(RegistryInner::default())),
            ttl,
        }
    }

    /// Get the global singleton instance.
    pub fn global() -> Self {
        static INSTANCE: std::sync::OnceLock<ModelRegistry> = std::sync::OnceLock::new();
        INSTANCE.get_or_init(|| ModelRegistry::new()).clone()
    }

    /// List all available models from all providers.
    /// Returns models in "{provider}/{model_id}" format with provider configuration.
    pub async fn list_all_models(&self, providers: &[ProviderConfig]) -> Vec<ModelEntry> {
        let mut all_models = Vec::new();

        for provider in providers {
            match self.fetch_or_get_cached(provider).await {
                Ok(models) => {
                    for model in models {
                        all_models.push(ModelEntry {
                            id: format!("{}/{}", provider.name, model.id),
                            name: model.id,
                            provider: provider.name.clone(),
                            base_url: provider.base_url.clone(),
                            api_key: provider.api_key.clone(),
                            provider_type: provider.provider_type.clone(),
                            ..Default::default()
                        });
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        provider = %provider.name,
                        error = %e,
                        "Failed to fetch models from provider"
                    );
                }
            }
        }

        tracing::info!(
            total_models = all_models.len(),
            "Listed all available models"
        );

        all_models
    }

    /// Get a specific model by its combined ID ("{provider}/{model_id}").
    /// Returns None if the model is not found or the provider doesn't exist.
    pub async fn get_model(&self, combined_id: &str, providers: &[ProviderConfig]) -> Option<ModelEntry> {
        let (provider_name, model_id) = combined_id.split_once('/')?;

        // Find the provider
        let provider = providers.iter().find(|p| p.name == provider_name)?;

        // Get models for this provider
        let models = self.fetch_or_get_cached(provider).await.ok()?;

        // Find the specific model
        let _model = models.iter().find(|m| m.id == model_id)?;

        Some(ModelEntry {
            id: combined_id.to_string(),
            name: model_id.to_string(),
            provider: provider.name.clone(),
            base_url: provider.base_url.clone(),
            api_key: provider.api_key.clone(),
            provider_type: provider.provider_type.clone(),
            ..Default::default()
        })
    }

    /// Fetch models from a provider, using cache if valid.
    async fn fetch_or_get_cached(&self, provider: &ProviderConfig) -> Result<Vec<ModelInfo>, AgentError> {
        // Check cache first
        {
            let inner = self.inner.read().await;
            if let Some(cached) = inner.cache.iter().find(|c| c.provider_name == provider.name) {
                if !cached.is_expired(self.ttl) {
                    return Ok(cached.models.clone());
                }
            }
        }

        // Fetch from provider
        let models = fetch_provider_models(
            provider.provider_type.as_deref(),
            provider.base_url.as_deref(),
            provider.api_key.as_deref(),
        )
        .await?;

        // Update cache
        {
            let mut inner = self.inner.write().await;
            // Remove old entry if exists
            inner.cache.retain(|c| c.provider_name != provider.name);
            // Add new entry
            inner.cache.push(CachedProviderModels {
                provider_name: provider.name.clone(),
                models: models.clone(),
                fetched_at: Instant::now(),
                base_url: provider.base_url.clone(),
                api_key: provider.api_key.clone(),
                provider_type: provider.provider_type.clone(),
            });
        }

        Ok(models)
    }

    /// Invalidate cache for a specific provider.
    pub async fn invalidate(&self, provider_name: &str) {
        let mut inner = self.inner.write().await;
        inner.cache.retain(|c| c.provider_name != provider_name);
    }

    /// Invalidate all cached models.
    pub async fn invalidate_all(&self) {
        let mut inner = self.inner.write().await;
        inner.cache.clear();
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Creates an LLM client from a ModelEntry with provider configuration.
///
/// This is a convenience function that creates the appropriate LLM client
/// (ChatOpenAI or ChatBigModel) based on the provider type in the ModelEntry.
/// It also applies runtime configuration like temperature and tool_choice.
///
/// # Example
///
/// ```ignore
/// use loom::llm::{create_llm_client, ModelEntry};
///
/// let entry = ModelEntry {
///     id: "openai/gpt-4o".to_string(),
///     name: "gpt-4o".to_string(),
///     provider: "openai".to_string(),
///     base_url: Some("https://api.openai.com/v1".to_string()),
///     api_key: Some("sk-test".to_string()),
///     provider_type: None,
///     temperature: Some(0.7),
///     max_tokens: None,
///     tool_choice: None,
/// };
/// let client = create_llm_client(&entry)?;
/// ```
pub fn create_llm_client(entry: &ModelEntry) -> Result<Box<dyn LlmClient>, AgentError> {
    let model = entry.name.clone();
    let provider_type = entry.provider_type.as_deref().unwrap_or("openai");

    let client: Box<dyn LlmClient> = match provider_type {
        "bigmodel" => {
            let base_url = entry
                .base_url
                .clone()
                .ok_or_else(|| AgentError::ExecutionFailed("base_url is required for bigmodel".to_string()))?;
            let api_key = entry
                .api_key
                .clone()
                .ok_or_else(|| AgentError::ExecutionFailed("api_key is required for bigmodel".to_string()))?;
            let mut client = ChatBigModel::with_config(base_url, api_key, model);
            if let Some(temp) = entry.temperature {
                client = client.with_temperature(temp);
            }
            if let Some(mode) = entry.tool_choice {
                client = client.with_tool_choice(mode);
            }
            Box::new(client)
        }
        _ => {
            // Default to OpenAI-compatible client
            let mut config = OpenAIConfig::new();
            if let Some(ref api_key) = entry.api_key {
                config = config.with_api_key(api_key);
            }
            if let Some(ref base_url) = entry.base_url {
                let base_url = base_url.trim_end_matches('/');
                config = config.with_api_base(base_url);
            }
            let mut client = ChatOpenAI::with_config(config, model);
            if let Some(temp) = entry.temperature {
                client = client.with_temperature(temp);
            }
            if let Some(mode) = entry.tool_choice {
                client = client.with_tool_choice(mode);
            }
            Box::new(client)
        }
    };

    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_all_models_returns_empty_for_no_providers() {
        let registry = ModelRegistry::new();
        let models = registry.list_all_models(&[]).await;
        assert!(models.is_empty());
    }

    #[test]
    fn test_provider_config_clone() {
        let config = ProviderConfig {
            name: "test".to_string(),
            base_url: Some("https://api.example.com".to_string()),
            api_key: Some("key".to_string()),
            provider_type: Some("openai".to_string()),
        };
        let cloned = config.clone();
        assert_eq!(config.name, cloned.name);
    }

    #[test]
    fn test_model_entry_fields() {
        let entry = ModelEntry {
            id: "openai/gpt-4o".to_string(),
            name: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            api_key: Some("sk-test".to_string()),
            provider_type: None,
            ..Default::default()
        };
        assert_eq!(entry.id, "openai/gpt-4o");
        assert_eq!(entry.name, "gpt-4o");
    }

    #[test]
    fn test_model_entry_new() {
        let entry = ModelEntry::new("openai", "gpt-4o");
        assert_eq!(entry.id, "openai/gpt-4o");
        assert_eq!(entry.name, "gpt-4o");
        assert_eq!(entry.provider, "openai");
    }

    #[test]
    fn test_model_entry_with_provider_config() {
        let entry = ModelEntry::new("openai", "gpt-4o")
            .with_base_url("https://api.openai.com/v1")
            .with_api_key("sk-test");
        
        assert_eq!(entry.id, "openai/gpt-4o");
        assert_eq!(entry.base_url, Some("https://api.openai.com/v1".to_string()));
        assert_eq!(entry.api_key, Some("sk-test".to_string()));
    }

    #[test]
    fn test_model_entry_with_runtime_config() {
        let entry = ModelEntry::new("openai", "gpt-4o")
            .with_temperature(0.7)
            .with_max_tokens(1000);
        
        assert_eq!(entry.id, "openai/gpt-4o");
        assert_eq!(entry.temperature, Some(0.7));
        assert_eq!(entry.max_tokens, Some(1000));
    }

    #[test]
    fn test_model_entry_from_provider_config() {
        let provider = ProviderConfig {
            name: "openai".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            api_key: Some("sk-test".to_string()),
            provider_type: None,
        };
        
        let entry = ModelEntry::from_provider_config(&provider, "gpt-4o");
        assert_eq!(entry.id, "openai/gpt-4o");
        assert_eq!(entry.name, "gpt-4o");
        assert_eq!(entry.provider, "openai");
        assert_eq!(entry.base_url, Some("https://api.openai.com/v1".to_string()));
        assert_eq!(entry.api_key, Some("sk-test".to_string()));
    }
}
