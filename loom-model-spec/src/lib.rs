//! Model limit resolver: query model context/output limits from models.dev, local files, or cache.
//!
//! See [DEVELOPMENT-PLAN.md](../../../docs/DEVELOPMENT-PLAN.md) for implementation phases.
//!
//! # Example
//!
//! ```ignore
//! use loom_model_spec::*;
//! use std::sync::Arc;
//!
//! let models_dev = CachedResolver::new(ModelsDevResolver::new());
//! let cached = Arc::new(models_dev);
//!
//! // Optional: preload cache at startup
//! if let Ok(specs) = cached.inner().fetch_all().await {
//!     cached.refresh(specs).await;
//! }
//! let refresher = ResolverRefresher::new(cached.clone(), std::time::Duration::from_secs(86400));
//! refresher.spawn();
//! ```

mod cached;
mod composite;
mod config_model;
mod config_override;
mod local_file;
mod models_dev;
mod refresher;
mod resolver;
mod spec;

pub use cached::CachedResolver;
pub use composite::CompositeResolver;
pub use config_model::{ConfigModelEntry, ConfigModelResolver, ConfigProviderEntry};
pub use config_override::ConfigOverride;
pub use local_file::LocalFileResolver;
pub use models_dev::{HttpClient, ModelsDevResolver, ReqwestHttpClient, DEFAULT_MODELS_DEV_URL};
pub use refresher::ResolverRefresher;
pub use resolver::ModelLimitResolver;
pub use spec::{Cost, Modalities, ModalityType, Model, ModelLimit, ModelSpec, ModelTier, Provider};

use std::sync::Arc;

/// Build a `CompositeResolver` with a standard priority chain.
///
/// Chain: `ConfigOverride` → `ConfigModelResolver` → `CachedResolver<ModelsDevResolver>`
///
/// Pass `config_providers` from `config.toml`'s `[[providers]]` section to enable
/// manual model spec overrides.
pub fn build_composite_resolver(
    config_override: Option<ConfigOverride>,
    config_providers: Vec<ConfigProviderEntry>,
) -> Arc<CompositeResolver> {
    let mut sources: Vec<Arc<dyn ModelLimitResolver>> = Vec::new();

    if let Some(cfg) = config_override {
        sources.push(Arc::new(cfg));
    }

    let config_model = ConfigModelResolver::from_providers(&config_providers);
    sources.push(Arc::new(config_model));

    let models_dev = ModelsDevResolver::new();
    let cached = CachedResolver::new(models_dev);
    sources.push(Arc::new(cached));

    Arc::new(CompositeResolver::new(sources))
}