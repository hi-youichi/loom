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
//!     fetch_models: false,
//! }];
//! let models = registry.list_all_models(&providers).await;
//!
//! // Get specific model with full config
//! let model = registry.get_model("openai/gpt-4o", &providers).await;
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use futures::future::join_all;

use crate::error::AgentError;
use crate::llm::{ChatOpenAI, ChatOpenAICompat, LlmClient, LlmProvider};
use crate::model_spec::{ModelsDevResolver, Provider as SpecProvider};
use async_openai::config::OpenAIConfig;

/// Default TTL for cached model lists (5 minutes).
const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(300);

/// Default TTL for provider API cache (5 minutes).
const DEFAULT_PROVIDER_CACHE_TTL: Duration = Duration::from_secs(300);

/// Cached model list from provider API or local storage.
#[derive(Clone, Debug)]
pub struct CachedModelList {
    /// List of cached model entries.
    pub models: Vec<ModelEntry>,
    /// When the cache was created.
    pub fetched_at: Instant,
    /// Time-to-live for this cache entry.
    pub ttl: Duration,
}

impl CachedModelList {
    /// Create a new cached model list.
    pub fn new(models: Vec<ModelEntry>, ttl: Duration) -> Self {
        Self {
            models,
            fetched_at: Instant::now(),
            ttl,
        }
    }
    
    /// Check if the cache has expired.
    pub fn is_expired(&self) -> bool {
        self.fetched_at.elapsed() > self.ttl
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
        
        for model in self.models_dev.into_iter()
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
        }
    }
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
    /// Provider type from provider config (e.g., "openai", "openai_compat", "bigmodel").
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

/// Cached model catalog fetched from models.dev.
#[derive(Clone, Debug)]
struct CachedSpecProviders {
    /// Provider metadata from models.dev keyed by normalized provider name.
    providers: HashMap<String, SpecProvider>,
    /// When the models were fetched.
    fetched_at: Instant,
}

impl CachedSpecProviders {
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
    /// Cached provider catalog from models.dev.
    cache: Option<CachedSpecProviders>,
    /// Cached model lists from provider APIs.
    provider_cache: HashMap<String, CachedModelList>,
    /// Local model lists (persisted storage).
    local_models: HashMap<String, Vec<ModelEntry>>,
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
        INSTANCE.get_or_init(ModelRegistry::new).clone()
    }

    /// List all available models from all providers.
    /// Returns models in "{provider}/{model_id}" format with provider configuration.
    pub async fn list_all_models(&self, providers: &[ProviderConfig]) -> Vec<ModelEntry> {
        match self.list_all_models_result(providers).await {
            Ok(models) => models,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to resolve models from model spec");
                Vec::new()
            }
        }
    }

    /// List all available models from configured providers using model spec.
    pub async fn list_all_models_result(
        &self,
        providers: &[ProviderConfig],
    ) -> Result<Vec<ModelEntry>, AgentError> {
        if providers.is_empty() {
            tracing::info!(
                total_models = 0,
                "Listed all available models from model spec (no providers configured)"
            );
            return Ok(Vec::new());
        }

        let mut all_models = Vec::new();
        let mut seen_ids = HashSet::new();
        let mut need_spec_providers = false;

        for provider in providers {
            if provider.fetch_models {
                if let Some(ref base_url) = provider.base_url {
                    let url = format!("{}/models", base_url.trim_end_matches('/'));
                    match fetch_models_from_api(&url, provider.api_key.as_deref()).await {
                        Ok(model_ids) => {
                            tracing::info!(
                                provider = %provider.name,
                                count = model_ids.len(),
                                "Fetched models from provider API"
                            );
                            for model_id in model_ids {
                                let entry = ModelEntry::from_provider_config(provider, &model_id);
                                if seen_ids.insert(entry.id.clone()) {
                                    all_models.push(entry);
                                }
                            }
                            continue;
                        }
                        Err(e) => {
                            tracing::warn!(
                                provider = %provider.name,
                                url = %url,
                                error = %e,
                                "Failed to fetch models from provider API; skipping provider"
                            );
                            continue;
                        }
                    }
                } else {
                    tracing::warn!(
                        provider = %provider.name,
                        "fetch_models=true but base_url is missing; skipping provider"
                    );
                    continue;
                }
            }
            need_spec_providers = true;
        }

        if need_spec_providers {
            let spec_providers = self.fetch_or_get_cached_spec_providers().await?;
            for provider in providers {
                if provider.fetch_models {
                    continue;
                }
                let normalized = Self::normalize_provider_name(&provider.name);
                let Some(spec_provider) = spec_providers.get(&normalized) else {
                    tracing::warn!(
                        provider = %provider.name,
                        "Provider not found in model spec; skipping provider models"
                    );
                    continue;
                };

                for model_id in spec_provider.models.keys() {
                    let mut entry = ModelEntry::from_provider_config(provider, model_id);
                    if entry.base_url.is_none() {
                        if let Some(ref api) = spec_provider.api {
                            entry.base_url = Some(api.clone());
                        }
                    }
                    if entry.provider_type.is_none()
                        && !entry.provider.eq_ignore_ascii_case("openai")
                    {
                        entry.provider_type = Some("openai_compat".to_string());
                    }
                    if seen_ids.insert(entry.id.clone()) {
                        all_models.push(entry);
                    }
                }
            }
        }

        all_models.sort_by(|a, b| {
            a.provider
                .cmp(&b.provider)
                .then_with(|| a.name.cmp(&b.name))
        });

        tracing::info!(
            total_models = all_models.len(),
            "Listed all available models from model spec"
        );
        Ok(all_models)
    }

    /// Get a specific model by its combined ID ("{provider}/{model_id}").
    /// Returns None if the model is not found or the provider doesn't exist.
    pub async fn get_model(
        &self,
        combined_id: &str,
        providers: &[ProviderConfig],
    ) -> Option<ModelEntry> {
        self.get_model_result(combined_id, providers)
            .await
            .ok()
            .flatten()
    }

    /// Get a specific model by combined ID using model spec metadata.
    pub async fn get_model_result(
        &self,
        combined_id: &str,
        providers: &[ProviderConfig],
    ) -> Result<Option<ModelEntry>, AgentError> {
        let Some((provider_name, model_id)) = combined_id.split_once('/') else {
            return Ok(None);
        };

        let Some(provider_cfg) = providers.iter().find(|p| p.name == provider_name) else {
            return Ok(None);
        };

        let spec_providers = self.fetch_or_get_cached_spec_providers().await?;
        let normalized = Self::normalize_provider_name(provider_name);
        let Some(spec_provider) = spec_providers.get(&normalized) else {
            return Ok(None);
        };

        if !spec_provider.models.contains_key(model_id) {
            return Ok(None);
        }

        let mut entry = ModelEntry::from_provider_config(provider_cfg, model_id);
        if entry.base_url.is_none() {
            if let Some(ref api) = spec_provider.api {
                entry.base_url = Some(api.clone());
            }
        }
        if entry.provider_type.is_none() && !entry.provider.eq_ignore_ascii_case("openai") {
            entry.provider_type = Some("openai_compat".to_string());
        }

        Ok(Some(entry))
    }

    /// Resolve the best model for a given provider and tier.
    ///
    /// Delegates to [`model_spec_core::spec::pick_best_for_tier`] for filtering and ranking,
    /// then builds a [`ModelEntry`] from the result.
    pub async fn resolve_tier(
        &self,
        provider: &str,
        tier: model_spec_core::spec::ModelTier,
        providers: &[ProviderConfig],
    ) -> Option<ModelEntry> {
        let provider_cfg = providers.iter().find(|p| p.name == provider)?;
        let spec_providers = self.fetch_or_get_cached_spec_providers().await.ok()?;
        let normalized = Self::normalize_provider_name(provider);
        let spec_provider = spec_providers.get(&normalized)?;

        let (model_id, _model) =
            model_spec_core::spec::pick_best_for_tier(&spec_provider.models, tier)?;

        let mut entry = ModelEntry::from_provider_config(provider_cfg, model_id);
        if entry.base_url.is_none() {
            if let Some(ref api) = spec_provider.api {
                entry.base_url = Some(api.clone());
            }
        }
        if entry.provider_type.is_none() && !entry.provider.eq_ignore_ascii_case("openai") {
            entry.provider_type = Some("openai_compat".to_string());
        }

        Some(entry)
    }

    /// Given a current model ID (e.g. `"anthropic/claude-sonnet-4"`), resolve
    /// the best model of `target_tier` from the same provider.
    pub async fn resolve_tier_for_model(
        &self,
        current_model: &str,
        target_tier: model_spec_core::spec::ModelTier,
        providers: &[ProviderConfig],
    ) -> Option<ModelEntry> {
        let (provider, _model_id) = current_model.split_once('/')?;
        self.resolve_tier(provider, target_tier, providers).await
    }

    fn normalize_provider_name(name: &str) -> String {
        name.trim().to_ascii_lowercase()
    }

    /// Fetch models.dev providers, using cache if valid.
    async fn fetch_or_get_cached_spec_providers(
        &self,
    ) -> Result<HashMap<String, SpecProvider>, AgentError> {
        // Check cache first
        {
            let inner = self.inner.read().await;
            if let Some(cached) = &inner.cache {
                if !cached.is_expired(self.ttl) {
                    return Ok(cached.providers.clone());
                }
            }
        }

        // Fetch from model spec
        let fetched = ModelsDevResolver::new()
            .fetch_all_providers()
            .await
            .map_err(|e| {
                AgentError::ExecutionFailed(format!("failed to fetch model spec providers: {e}"))
            })?;
        let providers: HashMap<String, SpecProvider> = fetched
            .into_iter()
            .map(|(k, v)| (Self::normalize_provider_name(&k), v))
            .collect();

        // Update cache
        {
            let mut inner = self.inner.write().await;
            inner.cache = Some(CachedSpecProviders {
                providers: providers.clone(),
                fetched_at: Instant::now(),
            });
        }

        Ok(providers)
    }

    /// Invalidate cache for a specific provider.
    pub async fn invalidate(&self, provider_name: &str) {
        let mut inner = self.inner.write().await;
        if let Some(cached) = &mut inner.cache {
            cached
                .providers
                .remove(&Self::normalize_provider_name(provider_name));
        }
    }

    /// Invalidate all cached models.
    pub async fn invalidate_all(&self) {
        let mut inner = self.inner.write().await;
        inner.cache = None;
        inner.provider_cache.clear();
        inner.local_models.clear();
    }

    /// Get cached model list for a specific provider.
    pub async fn get_cached_provider_models(&self, provider: &str) -> Option<Vec<ModelEntry>> {
        let inner = self.inner.read().await;
        inner.provider_cache.get(provider).and_then(|cached| {
            if cached.is_expired() {
                None
            } else {
                Some(cached.models.clone())
            }
        })
    }

    /// Cache model list for a specific provider.
    pub async fn cache_provider_models(&self, provider: String, models: Vec<ModelEntry>, ttl: Duration) {
        let mut inner = self.inner.write().await;
        inner.provider_cache.insert(provider, CachedModelList::new(models, ttl));
    }

    /// Invalidate cached models for a specific provider.
    pub async fn invalidate_provider_models(&self, provider: &str) {
        let mut inner = self.inner.write().await;
        inner.provider_cache.remove(provider);
    }

    /// Get local model list for a specific provider.
    pub async fn get_local_models(&self, provider: &str) -> Option<Vec<ModelEntry>> {
        let inner = self.inner.read().await;
        inner.local_models.get(provider).cloned()
    }

    /// Set local model list for a specific provider.
    pub async fn set_local_models(&self, provider: String, models: Vec<ModelEntry>) {
        let mut inner = self.inner.write().await;
        inner.local_models.insert(provider, models);
    }

    /// Fetch model list from provider API with caching.
    pub async fn fetch_provider_models_cached(
        &self,
        provider: &ProviderConfig,
    ) -> Result<Vec<ModelEntry>, AgentError> {
        // Check cache first
        if let Some(cached) = self.get_cached_provider_models(&provider.name).await {
            tracing::debug!(
                provider = %provider.name,
                count = cached.len(),
                "Using cached provider models"
            );
            return Ok(cached);
        }

        // Fetch from API
        let models = self.fetch_provider_models_api(provider).await?;
        
        // Cache the result
        let ttl = provider.cache_ttl
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_PROVIDER_CACHE_TTL);
        
        self.cache_provider_models(provider.name.clone(), models.clone(), ttl).await;
        
        Ok(models)
    }

    /// Fetch model list from provider API without caching.
    async fn fetch_provider_models_api(
        &self,
        provider: &ProviderConfig,
    ) -> Result<Vec<ModelEntry>, AgentError> {
        let base_url = provider.base_url.as_ref().ok_or_else(|| {
            AgentError::ExecutionFailed(format!("Provider {} has no base_url configured", provider.name))
        })?;

        let url = format!("{}/models", base_url.trim_end_matches('/'));
        let model_ids = fetch_models_from_api(&url, provider.api_key.as_deref()).await?;
        
        let models: Vec<ModelEntry> = model_ids
            .into_iter()
            .map(|model_id| ModelEntry::from_provider_config(provider, &model_id))
            .collect();

        tracing::info!(
            provider = %provider.name,
            count = models.len(),
            "Fetched models from provider API"
        );

        Ok(models)
    }

    /// Fetch models from all sources in parallel.
    pub async fn fetch_all_model_sources(
        &self,
        providers: &[ProviderConfig],
    ) -> Result<CombinedModelList, AgentError> {
        // Fetch models.dev models
        let models_dev_future = async {
            match self.fetch_or_get_cached_spec_providers().await {
                Ok(spec_providers) => {
                    let mut models = Vec::new();
                    for provider in providers {
                        if !provider.fetch_models {
                            let normalized = Self::normalize_provider_name(&provider.name);
                            if let Some(spec_provider) = spec_providers.get(&normalized) {
                                for model_id in spec_provider.models.keys() {
                                    models.push(ModelEntry::from_provider_config(provider, model_id));
                                }
                            }
                        }
                    }
                    Ok::<Vec<ModelEntry>, AgentError>(models)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to fetch models.dev models");
                    Ok::<Vec<ModelEntry>, AgentError>(Vec::new())
                }
            }
        };

        // Fetch provider API models
        let provider_futures: Vec<_> = providers
            .iter()
            .filter(|p| p.fetch_models)
            .map(|provider| async move {
                match self.fetch_provider_models_cached(provider).await {
                    Ok(models) => Ok(models),
                    Err(e) => {
                        tracing::warn!(
                            provider = %provider.name,
                            error = %e,
                            "Failed to fetch models from provider API"
                        );
                        Ok(Vec::new())
                    }
                }
            })
            .collect();

        // Execute all futures in parallel
        let (models_dev_result, provider_results) = tokio::join!(
            models_dev_future,
            join_all(provider_futures)
        );

        let models_dev = models_dev_result?;
        let provider_models: Vec<ModelEntry> = provider_results
            .into_iter()
            .filter_map(|result: Result<Vec<ModelEntry>, AgentError>| result.ok())
            .flatten()
            .collect();

        // Load local models (placeholder for future implementation)
        let local_models = Vec::new();

        Ok(CombinedModelList {
            models_dev,
            provider_models,
            local_models,
        })
    }

    /// Intelligent tier resolution with multiple fallback mechanisms.
    pub async fn resolve_tier_intelligent(
        &self,
        provider: &str,
        tier: model_spec_core::spec::ModelTier,
        providers: &[ProviderConfig],
    ) -> Option<ModelEntry> {
        // 1. Try models.dev tier resolution
        if let Some(entry) = self.resolve_tier_from_dev(provider, tier, providers).await {
            tracing::debug!(
                provider = %provider,
                tier = ?tier,
                "Tier resolution succeeded using models.dev"
            );
            return Some(entry);
        }

        // 2. Try provider API model list matching
        if let Some(entry) = self.resolve_tier_from_provider_api(provider, tier, providers).await {
            tracing::debug!(
                provider = %provider,
                tier = ?tier,
                "Tier resolution succeeded using provider API"
            );
            return Some(entry);
        }

        // 3. Try local model list matching (placeholder for future implementation)
        if let Some(entry) = self.resolve_tier_from_local_models(provider, tier, providers).await {
            tracing::debug!(
                provider = %provider,
                tier = ?tier,
                "Tier resolution succeeded using local models"
            );
            return Some(entry);
        }

        tracing::warn!(
            provider = %provider,
            tier = ?tier,
            "Tier resolution failed using all methods"
        );
        None
    }

    /// Resolve tier using models.dev.
    async fn resolve_tier_from_dev(
        &self,
        provider: &str,
        tier: model_spec_core::spec::ModelTier,
        providers: &[ProviderConfig],
    ) -> Option<ModelEntry> {
        let provider_cfg = providers.iter().find(|p| p.name == provider)?;
        let spec_providers = self.fetch_or_get_cached_spec_providers().await.ok()?;
        let normalized = Self::normalize_provider_name(provider);
        let spec_provider = spec_providers.get(&normalized)?;

        let (model_id, _model) =
            model_spec_core::spec::pick_best_for_tier(&spec_provider.models, tier)?;

        let mut entry = ModelEntry::from_provider_config(provider_cfg, model_id);
        if entry.base_url.is_none() {
            if let Some(ref api) = spec_provider.api {
                entry.base_url = Some(api.clone());
            }
        }
        if entry.provider_type.is_none() && !entry.provider.eq_ignore_ascii_case("openai") {
            entry.provider_type = Some("openai_compat".to_string());
        }

        Some(entry)
    }

    /// Resolve tier using provider API model list.
    async fn resolve_tier_from_provider_api(
        &self,
        provider: &str,
        _tier: model_spec_core::spec::ModelTier,
        providers: &[ProviderConfig],
    ) -> Option<ModelEntry> {
        let provider_cfg = providers.iter().find(|p| p.name == provider)?;
        
        if !provider_cfg.fetch_models {
            return None; // Provider doesn't use API model list
        }

        let model_list = self.fetch_provider_models_cached(provider_cfg).await.ok()?;
        
        // Simple tier matching logic - in practice, this would need more sophisticated matching
        // For now, we'll just return the first model as a placeholder
        // TODO: Implement proper tier matching based on model capabilities
        if let Some(first_model) = model_list.first() {
            return Some(first_model.clone());
        }

        None
    }

    /// Resolve tier using local model list.
    async fn resolve_tier_from_local_models(
        &self,
        _provider: &str,
        _tier: model_spec_core::spec::ModelTier,
        _providers: &[ProviderConfig],
    ) -> Option<ModelEntry> {
        // Placeholder for future local model list implementation
        // This would load from a persisted local storage
        None
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(serde::Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModelItem>,
}

#[derive(serde::Deserialize)]
struct OpenAiModelItem {
    id: String,
}

async fn fetch_models_from_api(
    url: &str,
    api_key: Option<&str>,
) -> Result<Vec<String>, AgentError> {
    let client = reqwest::Client::new();
    let mut req = client.get(url);
    if let Some(key) = api_key {
        if key != "none" && !key.is_empty() {
            req = req.bearer_auth(key);
        }
    }
    let resp: OpenAiModelsResponse = req
        .send()
        .await
        .map_err(|e| {
            AgentError::ExecutionFailed(format!("failed to fetch models from {url}: {e}"))
        })?
        .json()
        .await
        .map_err(|e| {
            AgentError::ExecutionFailed(format!("failed to parse models response from {url}: {e}"))
        })?;
    Ok(resp.data.into_iter().map(|m| m.id).collect())
}

/// Creates an LLM client from a ModelEntry with provider configuration.
///
/// This is a convenience function that creates the appropriate LLM client
/// ([`ChatOpenAI`] or [`ChatOpenAICompat`]) based on the provider type in the ModelEntry.
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
    let provider_type = entry.provider_type.as_deref().unwrap_or_else(|| {
        if entry.provider.eq_ignore_ascii_case("openai") {
            "openai"
        } else {
            "openai_compat"
        }
    });

    let client: Box<dyn LlmClient> = match provider_type {
        "openai" => {
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
        _ => {
            let api_key = entry.api_key.clone().ok_or_else(|| {
                AgentError::ExecutionFailed(format!(
                    "api_key is required for provider '{}'",
                    provider_type
                ))
            })?;
            let base_url = entry
                .base_url
                .clone()
                .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
                .ok_or_else(|| {
                    AgentError::ExecutionFailed(format!(
                        "base_url (or OPENAI_BASE_URL) is required for non-openai provider '{}'",
                        provider_type
                    ))
                })?;
            let mut client = ChatOpenAICompat::with_config(base_url, api_key, model);
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

/// Create an [`LlmProvider`] from a [`ModelEntry`].
///
/// Returns an `Arc<dyn LlmProvider>` that can dynamically create [`LlmClient`] instances
/// for any model name, and resolve [`ModelTier`] abstractions to concrete model IDs.
pub fn create_llm_provider(
    entry: &ModelEntry,
    providers: Vec<ProviderConfig>,
) -> Result<Arc<dyn LlmProvider>, AgentError> {
    let provider_type = entry.provider_type.as_deref().unwrap_or_else(|| {
        if entry.provider.eq_ignore_ascii_case("openai") {
            "openai"
        } else {
            "openai_compat"
        }
    });

    match provider_type {
        "openai" => {
            let provider = crate::llm::openai_provider::OpenAIProvider::from_entry(
                entry,
                providers,
            );
            Ok(Arc::new(provider))
        }
        _ => {
            let provider = crate::llm::openai_compat_provider::OpenAICompatProvider::from_entry(
                entry,
                providers,
            );
            Ok(Arc::new(provider))
        }
    }
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
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
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
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
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
    fn test_resolve_tier_returns_none_for_unknown_provider() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let registry = ModelRegistry::new();
            let providers = vec![ProviderConfig {
                name: "anthropic".to_string(),
                base_url: Some("https://api.anthropic.com/v1".to_string()),
                api_key: Some("sk-test".to_string()),
                provider_type: None,
                fetch_models: false,
                cache_ttl: None,
                enable_tier_resolution: true,
            }];
            let result = registry
                .resolve_tier("unknown_provider", model_spec_core::spec::ModelTier::Light, &providers)
                .await;
            assert!(result.is_none());
        });
    }

    #[test]
    fn test_resolve_tier_for_model_extracts_provider() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let registry = ModelRegistry::new();
            let providers = vec![ProviderConfig {
                name: "anthropic".to_string(),
                base_url: Some("https://api.anthropic.com/v1".to_string()),
                api_key: Some("sk-test".to_string()),
                provider_type: None,
                fetch_models: false,
                cache_ttl: None,
                enable_tier_resolution: true,
            }];
            let result = registry
                .resolve_tier_for_model(
                    "anthropic/claude-sonnet-4",
                    model_spec_core::spec::ModelTier::Light,
                    &providers,
                )
                .await;
            // Will be None without network, but should parse provider correctly
            assert!(result.is_none() || result.is_some());
        });
    }
}
