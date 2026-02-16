//! Cached resolver: in-memory cache wrapper for any ModelLimitResolver.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::resolver::ModelLimitResolver;
use super::spec::ModelSpec;

/// Wraps any resolver with an in-memory cache.
pub struct CachedResolver<R> {
    inner: R,
    cache: Arc<RwLock<HashMap<String, ModelSpec>>>,
}

impl<R> CachedResolver<R>
where
    R: ModelLimitResolver,
{
    /// Create a new cached resolver.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Refresh cache with new specs. Merges into existing cache.
    pub async fn refresh(&self, specs: HashMap<String, ModelSpec>) {
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
impl<R> ModelLimitResolver for CachedResolver<R>
where
    R: ModelLimitResolver + Send + Sync,
{
    async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<ModelSpec> {
        let key = format!("{}/{}", provider_id, model_id);
        {
            let cache = self.cache.read().await;
            if let Some(spec) = cache.get(&key).cloned() {
                return Some(spec);
            }
        }
        let spec = self.inner.resolve(provider_id, model_id).await?;
        {
            let mut cache = self.cache.write().await;
            cache.insert(key, spec.clone());
        }
        Some(spec)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::model_spec::models_dev::{HttpClient, ModelsDevResolver};

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

    #[tokio::test]
    async fn cache_hits_avoid_inner_calls() {
        let body =
            r#"{"zai":{"models":{"glm-5":{"limit":{"context":204800,"output":131072}}}}}"#
                .to_string();
        let client = Arc::new(CountingMockClient {
            body,
            call_count: AtomicUsize::new(0),
        });
        let models_dev =
            ModelsDevResolver::with_client("https://x.com/api.json".to_string(), client.clone());
        let cached = CachedResolver::new(models_dev);

        let spec1 = cached.resolve("zai", "glm-5").await.unwrap();
        assert_eq!(spec1.context_limit, 204_800);
        assert_eq!(client.call_count.load(Ordering::SeqCst), 1);

        let spec2 = cached.resolve("zai", "glm-5").await.unwrap();
        assert_eq!(spec2.context_limit, 204_800);
        assert_eq!(client.call_count.load(Ordering::SeqCst), 1);
    }
}
