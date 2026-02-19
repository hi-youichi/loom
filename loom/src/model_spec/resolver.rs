//! Model limit resolver trait for querying model specifications.

use async_trait::async_trait;

use super::spec::ModelSpec;

/// Resolves model specifications (context limit, output limit) by provider and model id.
///
/// Implementations may fetch from remote APIs (e.g., models.dev), read from local files,
/// or serve from in-memory cache.
#[async_trait]
pub trait ModelLimitResolver: Send + Sync {
    /// Resolve model spec for the given provider and model.
    ///
    /// Returns `None` if the model is unknown or resolution fails.
    async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<ModelSpec>;
}
