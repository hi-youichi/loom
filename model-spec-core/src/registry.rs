//! Model registry types: ProviderConfig, ModelEntry, CachedModelList.

use crate::tool_choice::ToolChoiceMode;

/// Default TTL for cached model lists (5 minutes).
pub const DEFAULT_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(300);

/// Default TTL for provider API cache (5 minutes).
pub const DEFAULT_PROVIDER_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(300);

/// Cached model list from provider API or local storage.
#[derive(Clone, Debug)]
pub struct CachedModelList {
    /// List of cached model entries.
    pub models: Vec<ModelEntry>,
    /// When the cache was created.
    pub fetched_at: std::time::Instant,
    /// Time-to-live for this cache entry.
    pub ttl: std::time::Duration,
}

impl CachedModelList {
    /// Create a new cached model list.
    pub fn new(models: Vec<ModelEntry>, ttl: std::time::Duration) -> Self {
        Self {
            models,
            fetched_at: std::time::Instant::now(),
            ttl,
        }
    }

    /// Check if the cache has expired.
    pub fn is_expired(&self) -> bool {
        self.fetched_at.elapsed() >= self.ttl
    }
}

/// Combined model list from multiple sources.
#[derive(Clone, Debug, Default)]
pub struct CombinedModelList {
    /// Models from models.dev registry.
    pub models_dev: Vec<ModelEntry>,
    /// Models from provider APIs.
    pub provider_models: Vec<ModelEntry>,
    /// Models from local storage.
    pub local_models: Vec<ModelEntry>,
}

impl CombinedModelList {
    /// Merge all model lists, removing duplicates.
    pub fn merge_all(self) -> Vec<ModelEntry> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        for model in self
            .models_dev
            .into_iter()
            .chain(self.provider_models)
            .chain(self.local_models)
        {
            if seen.insert(model.id.clone()) {
                result.push(model);
            }
        }

        result
    }
}

// ============================================================================
// ProviderConfig
// ============================================================================

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
    /// Provider type: "openai" (default), "openai_compat", or "bigmodel" (alias).
    pub provider_type: Option<String>,
    /// When `true`, fetch model list from `{base_url}/models` instead of models.dev.
    pub fetch_models: bool,
    /// Cache TTL for provider API models (in seconds). Default: 300 seconds (5 minutes).
    pub cache_ttl: Option<u64>,
    /// When `true`, enable tier resolution for this provider. Default: `true`.
    pub enable_tier_resolution: bool,
    /// Model IDs declared in `config.toml` via `[[providers.models]]`.
    pub declared_models: Vec<String>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            base_url: None,
            api_key: None,
            provider_type: None,
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
            declared_models: Vec::new(),
        }
    }
}

// ============================================================================
// ModelEntry
// ============================================================================

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
    /// Provider type from provider config (e.g., "openai", "openai_compat", "bigmodel").
    pub provider_type: Option<String>,

    // === Runtime Configuration ===
    /// Sampling temperature (0.0 - 2.0).
    pub temperature: Option<f32>,
    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,
    /// Tool choice mode (auto, none, required).
    pub tool_choice: Option<ToolChoiceMode>,

    // === Model Selection ===
    /// Model family (e.g., "glm", "gpt-5.5").
    pub family: Option<String>,
    /// Model version/generation (e.g., "5", "latest").
    pub version: Option<String>,
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

    /// Set the model family.
    pub fn with_family(mut self, family: impl Into<String>) -> Self {
        self.family = Some(family.into());
        self
    }

    /// Set the model version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set the max tokens.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set the tool choice mode.
    pub fn with_tool_choice(mut self, tool_choice: ToolChoiceMode) -> Self {
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            entry.base_url,
            Some("https://api.openai.com/v1".to_string())
        );
        assert_eq!(entry.api_key, Some("sk-test".to_string()));
    }

    #[test]
    fn test_model_entry_with_runtime_config() {
        let entry = ModelEntry::new("openai", "gpt-4o")
            .with_temperature(0.7)
            .with_max_tokens(1000);

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
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
            declared_models: Vec::new(),
        };

        let entry = ModelEntry::from_provider_config(&provider, "gpt-4o");
        assert_eq!(entry.id, "openai/gpt-4o");
        assert_eq!(entry.name, "gpt-4o");
        assert_eq!(entry.provider, "openai");
        assert_eq!(
            entry.base_url,
            Some("https://api.openai.com/v1".to_string())
        );
        assert_eq!(entry.api_key, Some("sk-test".to_string()));
    }

    #[test]
    fn test_provider_config_default() {
        let config = ProviderConfig::default();
        assert!(config.name.is_empty());
        assert!(config.base_url.is_none());
        assert!(config.api_key.is_none());
        assert!(config.enable_tier_resolution);
        assert!(!config.fetch_models);
    }

    #[test]
    fn test_cached_model_list_expired() {
        let list = CachedModelList::new(vec![], std::time::Duration::from_secs(0));
        // TTL is 0, should be expired immediately
        assert!(list.is_expired());
    }
}
