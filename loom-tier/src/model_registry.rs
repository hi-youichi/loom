//! Unified model registry for all providers.
//!
//! Provides a single source of truth for all available models across providers.
//! Combines provider configuration with model lists to return fully resolved model entries.
//!
//! `create_llm_client` / `create_llm_provider` have been moved to `loom::llm_factory`.

pub use model_spec_core::registry::{
    CachedModelList, CombinedModelList, ModelEntry, ProviderConfig,
    DEFAULT_CACHE_TTL, DEFAULT_PROVIDER_CACHE_TTL,
};

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use loom_graph::GraphError;
use model_spec_core::Provider as SpecProvider;

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
    ) -> Result<Vec<ModelEntry>, GraphError> {
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

        for provider in providers {
            for model_id in &provider.declared_models {
                let entry = ModelEntry::from_provider_config(provider, model_id);
                if seen_ids.insert(entry.id.clone()) {
                    all_models.push(entry);
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
    ) -> Result<Option<ModelEntry>, GraphError> {
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

    pub async fn find_provider_data<'a>(
        &self,
        provider_name: &str,
        providers: &'a [ProviderConfig],
    ) -> Option<(&'a ProviderConfig, SpecProvider)> {
        let provider_cfg = providers.iter().find(|p| p.name == provider_name)?;
        let spec_providers = self.fetch_or_get_cached_spec_providers().await.ok()?;
        let normalized = Self::normalize_provider_name(provider_name);
        let spec_provider = spec_providers.get(&normalized)?;
        Some((provider_cfg, spec_provider.clone()))
    }

    pub fn normalize_provider_name(name: &str) -> String {
        name.trim().to_ascii_lowercase()
    }

    async fn fetch_or_get_cached_spec_providers(
        &self,
    ) -> Result<HashMap<String, SpecProvider>, GraphError> {
        {
            let inner = self.inner.read().await;
            if let Some(cached) = &inner.cache {
                if !cached.is_expired(self.ttl) {
                    return Ok(cached.providers.clone());
                }
            }
        }

        let fetched = model_spec_core::resolver::ModelsDevResolver::new()
            .fetch_all_providers()
            .await
            .map_err(|e| {
                GraphError::ExecutionFailed(format!("failed to fetch model spec providers: {e}"))
            })?;
        let providers: HashMap<String, SpecProvider> = fetched
            .into_iter()
            .map(|(k, v)| (Self::normalize_provider_name(&k), v))
            .collect();

        {
            let mut inner = self.inner.write().await;
            inner.cache = Some(CachedSpecProviders {
                providers: providers.clone(),
                fetched_at: Instant::now(),
            });
        }

        Ok(providers)
    }

    pub async fn invalidate(&self, provider_name: &str) {
        let mut inner = self.inner.write().await;
        if let Some(cached) = &mut inner.cache {
            cached
                .providers
                .remove(&Self::normalize_provider_name(provider_name));
        }
    }

    pub async fn invalidate_all(&self) {
        let mut inner = self.inner.write().await;
        inner.cache = None;
        inner.provider_cache.clear();
        inner.local_models.clear();
    }

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

    pub async fn cache_provider_models(&self, provider: String, models: Vec<ModelEntry>, ttl: Duration) {
        let mut inner = self.inner.write().await;
        inner.provider_cache.insert(provider, CachedModelList::new(models, ttl));
    }

    pub async fn invalidate_provider_models(&self, provider: &str) {
        let mut inner = self.inner.write().await;
        inner.provider_cache.remove(provider);
    }

    pub async fn get_local_models(&self, provider: &str) -> Option<Vec<ModelEntry>> {
        let inner = self.inner.read().await;
        inner.local_models.get(provider).cloned()
    }

    pub async fn set_local_models(&self, provider: String, models: Vec<ModelEntry>) {
        let mut inner = self.inner.write().await;
        inner.local_models.insert(provider, models);
    }

    pub async fn fetch_provider_models_cached(
        &self,
        provider: &ProviderConfig,
    ) -> Result<Vec<ModelEntry>, GraphError> {
        if let Some(cached) = self.get_cached_provider_models(&provider.name).await {
            tracing::debug!(
                provider = %provider.name,
                count = cached.len(),
                "Using cached provider models"
            );
            return Ok(cached);
        }

        let models = self.fetch_provider_models_api(provider).await?;

        let ttl = provider.cache_ttl
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_PROVIDER_CACHE_TTL);

        self.cache_provider_models(provider.name.clone(), models.clone(), ttl).await;

        Ok(models)
    }

    async fn fetch_provider_models_api(
        &self,
        provider: &ProviderConfig,
    ) -> Result<Vec<ModelEntry>, GraphError> {
        let base_url = provider.base_url.as_ref().ok_or_else(|| {
            GraphError::ExecutionFailed(format!("Provider {} has no base_url configured", provider.name))
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
) -> Result<Vec<String>, GraphError> {
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
            GraphError::ExecutionFailed(format!("failed to fetch models from {url}: {e}"))
        })?
        .json()
        .await
        .map_err(|e| {
            GraphError::ExecutionFailed(format!("failed to parse models response from {url}: {e}"))
        })?;
    Ok(resp.data.into_iter().map(|m| m.id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
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
            declared_models: Vec::new(),
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
}
