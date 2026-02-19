//! Model limit resolver: query model context/output limits from models.dev, local files, or cache.
//!
//! See [DEVELOPMENT-PLAN.md](../../../docs/DEVELOPMENT-PLAN.md) for implementation phases.
//!
//! # Example
//!
//! ```ignore
//! use loom::model_spec::*;
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
mod config_override;
mod local_file;
mod models_dev;
mod refresher;
mod resolver;
mod spec;

pub use cached::CachedResolver;
pub use composite::CompositeResolver;
pub use config_override::ConfigOverride;
pub use local_file::LocalFileResolver;
pub use models_dev::{HttpClient, ModelsDevResolver, ReqwestHttpClient, DEFAULT_MODELS_DEV_URL};
pub use refresher::ResolverRefresher;
pub use resolver::ModelLimitResolver;
pub use spec::ModelSpec;
