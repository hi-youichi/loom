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
    pub fn new(
        cached: Arc<CachedResolver<ModelsDevResolver>>,
        interval: Duration,
    ) -> Self {
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
