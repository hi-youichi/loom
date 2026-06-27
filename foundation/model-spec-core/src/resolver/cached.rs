//! Cached resolver: in-memory cache wrapper for any ModelResolver.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::ModelResolver;
use crate::Model;

/// Wraps any resolver with an in-memory cache.
pub struct CachedResolver<R> {
    inner: R,
    cache: Arc<RwLock<HashMap<String, Model>>>,
}

impl<R> CachedResolver<R>
where
    R: ModelResolver,
{
    /// Create a new cached resolver.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Refresh cache with new specs. Merges into existing cache.
    pub async fn refresh(&self, specs: HashMap<String, Model>) {
        let mut cache = self.cache.write().await;
        for (k, v) in specs {
            cache.insert(k, v);
        }
    }

    /// Clear the cache.
    pub async fn clear(&self) {
        self.cache.write().await.clear();
    }

    /// Get reference to inner resolver.
    pub fn inner(&self) -> &R {
        &self.inner
    }
}

#[async_trait]
impl<R> ModelResolver for CachedResolver<R>
where
    R: ModelResolver + Send + Sync,
{
    async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<Model> {
        let key = format!("{}/{}", provider_id, model_id);
        {
            let cache = self.cache.read().await;
            if let Some(model) = cache.get(&key).cloned() {
                return Some(model);
            }
        }
        let model = self.inner.resolve(provider_id, model_id).await?;
        {
            let mut cache = self.cache.write().await;
            cache.insert(key, model.clone());
        }
        Some(model)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::resolver::models_dev::{HttpClient, ModelsDevResolver};
    use crate::ModelLimit;

    struct CountingMockClient {
        body: String,
        call_count: AtomicUsize,
    }

    #[async_trait]
    impl HttpClient for CountingMockClient {
        async fn get(&self, _url: &str) -> Result<String, String> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(self.body.clone())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cache_hits_avoid_inner_calls() {
        let body = r#"{"zai":{"models":{"glm-5":{"limit":{"context":204800,"output":131072}}}}}"#
            .to_string();
        let client = Arc::new(CountingMockClient {
            body,
            call_count: AtomicUsize::new(0),
        });
        let models_dev =
            ModelsDevResolver::with_client("https://x.com/api.json".to_string(), client.clone());
        let cached = CachedResolver::new(models_dev);

        let model1 = cached.resolve("zai", "glm-5").await.unwrap();
        assert_eq!(model1.limit.context, 204_800);
        assert_eq!(client.call_count.load(Ordering::SeqCst), 1);

        let model2 = cached.resolve("zai", "glm-5").await.unwrap();
        assert_eq!(model2.limit.context, 204_800);
        assert_eq!(client.call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_combined_uses_cache() {
        let body =
            r#"{"openai":{"models":{"gpt-4o":{"limit":{"context":128000,"output":16384}}}}}"#
                .to_string();
        let client = Arc::new(CountingMockClient {
            body,
            call_count: AtomicUsize::new(0),
        });
        let models_dev =
            ModelsDevResolver::with_client("https://x.com/api.json".to_string(), client.clone());
        let cached = CachedResolver::new(models_dev);

        let model1 = cached.resolve_combined("openai/gpt-4o").await.unwrap();
        assert_eq!(model1.limit.context, 128_000);
        assert_eq!(client.call_count.load(Ordering::SeqCst), 1);

        let model2 = cached.resolve_combined("openai/gpt-4o").await.unwrap();
        assert_eq!(model2.limit.context, 128_000);
        assert_eq!(client.call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn refresh_populates_cache() {
        let cached = CachedResolver::new(ModelsDevResolver::new());
        let mut specs = HashMap::new();
        specs.insert(
            "test/model".to_string(),
            Model::minimal("model", ModelLimit::new(999_999, 1_000)),
        );
        cached.refresh(specs).await;

        let model = cached.resolve("test", "model").await.unwrap();
        assert_eq!(model.limit.context, 999_999);
    }
}
