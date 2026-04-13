//! Model list caching and provider query service.
//!
//! This module provides:
//! - [`ModelCache`]: In-memory cache for model lists with TTL support
//! - [`ModelFetcher`]: Service to query models from multiple providers
//!
//! # Example
//!
//! ```ignore
//! use loom::llm::{ModelCache, ModelFetcher};
//!
//! let cache = ModelCache::default();
//! let fetcher = ModelFetcher::new(cache);
//!
//! // List models from all configured providers
//! let models = fetcher.list_all_models(&providers).await;
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use crate::error::AgentError;
use crate::llm::{LlmClient, ModelInfo};

/// Default TTL for cached model lists (5 minutes).
const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(300);

/// In-memory cache for model lists per provider.
///
/// Uses `RwLock` for concurrent read access with TTL-based expiration.
#[derive(Clone)]
pub struct ModelCache {
    inner: Arc<RwLock<HashMap<String, CacheEntry>>>,
    ttl: Duration,
}

/// Cached entry with timestamp for TTL checking.
#[derive(Clone)]
struct CacheEntry {
    models: Vec<ModelInfo>,
    fetched_at: Instant,
}

impl ModelCache {
    /// Creates a new cache with custom TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }

    /// Gets cached models if not expired, otherwise fetches fresh data.
    ///
    /// The `fetch_fn` is only called when the cache misses or the entry expired.
    pub async fn get_or_fetch<F, Fut>(
        &self,
        provider_name: &str,
        fetch_fn: F,
    ) -> Result<Vec<ModelInfo>, AgentError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<ModelInfo>, AgentError>>,
    {
        // Check cache first (read lock)
        {
            let cache = self.inner.read().await;
            if let Some(entry) = cache.get(provider_name) {
                if entry.fetched_at.elapsed() < self.ttl {
                    return Ok(entry.models.clone());
                }
            }
        }

        // Fetch fresh data
        let models: Vec<ModelInfo> = fetch_fn().await?;

        // Update cache (write lock)
        {
            let mut cache = self.inner.write().await;
            cache.insert(
                provider_name.to_string(),
                CacheEntry {
                    models: models.clone(),
                    fetched_at: Instant::now(),
                },
            );
        }

        Ok(models)
    }

    /// Invalidates cache for a specific provider.
    pub async fn invalidate(&self, provider_name: &str) {
        let mut cache = self.inner.write().await;
        cache.remove(provider_name);
    }

    /// Clears all cached entries.
    pub async fn clear(&self) {
        let mut cache = self.inner.write().await;
        cache.clear();
    }

    /// Returns the number of cached entries (including potentially expired ones).
    pub async fn len(&self) -> usize {
        let cache = self.inner.read().await;
        cache.len()
    }

    /// Returns true if cache is empty.
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

impl Default for ModelCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_TTL)
    }
}

/// Result of fetching models from all providers.
#[derive(Debug)]
pub struct ProviderModels {
    /// Provider name.
    pub provider: String,
    /// Available models from this provider.
    pub models: Vec<ModelInfo>,
    /// Error message if fetch failed.
    pub error: Option<String>,
}

impl ProviderModels {
    /// Creates a successful result.
    pub fn ok(provider: String, models: Vec<ModelInfo>) -> Self {
        Self {
            provider,
            models,
            error: None,
        }
    }

    /// Creates a failed result.
    pub fn err(provider: String, error: String) -> Self {
        Self {
            provider,
            models: vec![],
            error: Some(error),
        }
    }

    /// Returns true if this result contains models.
    pub fn has_models(&self) -> bool {
        !self.models.is_empty()
    }
}

/// Fetches models from a single provider using the appropriate LLM client.
///
/// This function creates a temporary LLM client based on the provider type
/// and calls its `list_models()` method.
///
/// # Arguments
///
/// * `provider_type` - Provider type string ("openai", "bigmodel", etc.)
/// * `base_url` - API base URL
/// * `api_key` - API key for authentication
///
/// # Returns
///
/// A `Result` containing a vector of `ModelInfo` on success, or an `AgentError` on failure.
pub async fn fetch_provider_models(
    provider_type: Option<&str>,
    base_url: Option<&str>,
    api_key: Option<&str>,
) -> Result<Vec<ModelInfo>, AgentError> {
    let provider_type = provider_type.unwrap_or("openai");
    let base_url = base_url.unwrap_or("https://api.openai.com/v1");
    let api_key = api_key.unwrap_or("");

    match provider_type {
        "openai" => {
            use crate::llm::ChatOpenAI;
            use async_openai::config::OpenAIConfig;

            let config = OpenAIConfig::new()
                .with_api_key(api_key)
                .with_api_base(base_url);
            let client = ChatOpenAI::with_config(config, "dummy-model");
            client.list_models().await
        }
        _ => {
            use crate::llm::ChatOpenAICompat;
            let client = ChatOpenAICompat::with_config(base_url, api_key, "dummy-model");
            client.list_models().await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_hit() {
        let cache = ModelCache::new(Duration::from_secs(60));
        let models = vec![ModelInfo {
            id: "gpt-4".to_string(),
            created: None,
            owned_by: None,
        }];

        let result = cache
            .get_or_fetch("openai", || async { Ok(models.clone()) })
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(cache.len().await, 1);
    }

    #[tokio::test]
    async fn test_cache_expiry() {
        let cache = ModelCache::new(Duration::from_millis(10));
        let models = vec![ModelInfo {
            id: "gpt-4".to_string(),
            created: None,
            owned_by: None,
        }];

        // First fetch
        cache
            .get_or_fetch("openai", || async { Ok(models.clone()) })
            .await
            .unwrap();

        // Wait for expiry
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Should refetch
        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let count = call_count.clone();
        cache
            .get_or_fetch("openai", move || {
                let count = count.clone();
                async move {
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(models.clone())
                }
            })
            .await
            .unwrap();

        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_cache_invalidate() {
        let cache = ModelCache::new(Duration::from_secs(60));
        let models = vec![ModelInfo {
            id: "gpt-4".to_string(),
            created: None,
            owned_by: None,
        }];

        cache
            .get_or_fetch("openai", || async { Ok(models.clone()) })
            .await
            .unwrap();

        assert_eq!(cache.len().await, 1);

        cache.invalidate("openai").await;
        assert_eq!(cache.len().await, 0);
    }
}
