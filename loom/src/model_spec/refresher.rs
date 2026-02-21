//! Background refresher: periodically fetches models.dev and updates cache.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;

use super::cached::CachedResolver;
use super::models_dev::ModelsDevResolver;

/// Spawns a background task that periodically refreshes the cache from models.dev.
pub struct ResolverRefresher {
    cached: Arc<CachedResolver<ModelsDevResolver>>,
    interval: Duration,
}

impl ResolverRefresher {
    /// Create a refresher that will run every `interval`.
    pub fn new(cached: Arc<CachedResolver<ModelsDevResolver>>, interval: Duration) -> Self {
        Self { cached, interval }
    }

    /// Spawn the background refresh loop. Returns a handle that can be used to abort.
    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if let Ok(specs) = self.cached.inner().fetch_all().await {
                    self.cached.refresh(specs).await;
                    tracing::debug!("model_spec cache refreshed from models.dev");
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use crate::model_spec::models_dev::HttpClient;
    use crate::model_spec::resolver::ModelLimitResolver;

    struct CountingHttpClient {
        body: String,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl HttpClient for CountingHttpClient {
        async fn get(&self, _url: &str) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.body.clone())
        }
    }

    fn fixture_json() -> String {
        r#"{"zai":{"models":{"glm-5":{"limit":{"context":204800,"output":131072}}}}}"#.to_string()
    }

    #[tokio::test]
    async fn spawn_refreshes_periodically_and_can_be_aborted() {
        let client = Arc::new(CountingHttpClient {
            body: fixture_json(),
            calls: AtomicUsize::new(0),
        });
        let resolver = ModelsDevResolver::with_client(
            "https://example.com/models.json".to_string(),
            client.clone(),
        );
        let cached = Arc::new(CachedResolver::new(resolver));
        let refresher = ResolverRefresher::new(cached.clone(), Duration::from_millis(10));

        let handle = refresher.spawn();
        tokio::time::sleep(Duration::from_millis(35)).await;
        handle.abort();
        let _ = handle.await;

        assert!(client.calls.load(Ordering::SeqCst) >= 1);
        let spec = cached.resolve("zai", "glm-5").await.unwrap();
        assert_eq!(spec.context_limit, 204_800);
    }
}
