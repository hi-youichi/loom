//! Refresher: periodically refresh a CachedResolver's cache in a background task.

use std::sync::Arc;
use std::time::Duration;

use super::{CachedResolver, ModelsDevResolver};

/// Spawns a background task that periodically refreshes the cache.
pub struct ResolverRefresher {
    cached: Arc<CachedResolver<ModelsDevResolver>>,
    interval: Duration,
}

impl ResolverRefresher {
    /// Create a new refresher.
    pub fn new(cached: Arc<CachedResolver<ModelsDevResolver>>, interval: Duration) -> Self {
        Self { cached, interval }
    }

    /// Spawn the background refresh task. Returns the join handle.
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(self.interval);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if let Ok(specs) = self.cached.inner().fetch_all().await {
                    self.cached.refresh(specs).await;
                    tracing::debug!("models.dev cache refreshed");
                } else {
                    tracing::warn!("models.dev cache refresh failed");
                }
            }
        })
    }
}
