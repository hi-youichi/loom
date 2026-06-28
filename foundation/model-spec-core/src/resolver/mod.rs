//! Model resolver: query model specifications from models.dev, local files, config, or cache.

mod cached;
mod composite;
mod config_model;
mod config_override;
mod models_dev;

pub use cached::CachedResolver;
pub use composite::CompositeResolver;
pub use config_model::{ConfigModelEntry, ConfigProviderEntry};
pub use config_override::ConfigOverride;
pub use models_dev::{HttpClient, ModelsDevResolver, ReqwestHttpClient, DEFAULT_MODELS_DEV_URL};

use std::sync::Arc;

use async_trait::async_trait;

use crate::Model;

/// Resolves model specifications by provider and model id.
///
/// Implementations may fetch from remote APIs (e.g., models.dev), read from local files,
/// or serve from in-memory cache.
#[async_trait]
pub trait ModelResolver: Send + Sync {
    /// Resolve model for the given provider and model.
    ///
    /// Returns `None` if the model is unknown or resolution fails.
    async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<Model>;

    /// Resolve model from a combined string "provider/model".
    ///
    /// # Examples
    /// - `"openai/gpt-4o"` -> provider="openai", model="gpt-4o"
    /// - `"anthropic/claude-sonnet-4"` -> provider="anthropic", model="claude-sonnet-4"
    /// - `"google/gemini-2.5-pro"` -> provider="google", model="gemini-2.5-pro"
    ///
    /// Returns `None` if the string doesn't contain '/' or model not found.
    async fn resolve_combined(&self, model: &str) -> Option<Model> {
        let (provider, model_id) = split_provider_model(model)?;
        self.resolve(provider, model_id).await
    }
}

/// Split "provider/model" into (provider, model).
/// Handles model IDs like "openai/gpt-4o" and "zenmux/openai/gpt-5".
fn split_provider_model(model: &str) -> Option<(&str, &str)> {
    let slash_idx = model.find('/')?;
    let provider = &model[..slash_idx];
    let model_id = &model[slash_idx + 1..];
    if provider.is_empty() || model_id.is_empty() {
        return None;
    }
    Some((provider, model_id))
}

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
    let mut sources: Vec<Arc<dyn ModelResolver>> = Vec::new();

    if let Some(cfg) = config_override {
        sources.push(Arc::new(cfg));
    }

    let config_model = config_model::ConfigModelResolver::from_providers(&config_providers);
    sources.push(Arc::new(config_model));

    let models_dev = ModelsDevResolver::new();
    let cached = CachedResolver::new(models_dev);
    sources.push(Arc::new(cached));

    Arc::new(CompositeResolver::new(sources))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModelLimit;

    struct MockResolver;

    #[async_trait]
    impl ModelResolver for MockResolver {
        async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<Model> {
            if provider_id == "openai" && model_id == "gpt-4o" {
                Some(Model::minimal(model_id, ModelLimit::new(128_000, 16_384)))
            } else if provider_id == "zenmux" && model_id == "openai/gpt-5" {
                Some(Model::minimal(model_id, ModelLimit::new(400_000, 64_000)))
            } else {
                None
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_combined_splits_provider_and_model() {
        let resolver = MockResolver;
        let model = resolver.resolve_combined("openai/gpt-4o").await.unwrap();
        assert_eq!(model.limit.context, 128_000);
        assert_eq!(model.limit.output, 16_384);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_combined_handles_nested_model_id() {
        let resolver = MockResolver;
        let model = resolver
            .resolve_combined("zenmux/openai/gpt-5")
            .await
            .unwrap();
        assert_eq!(model.limit.context, 400_000);
        assert_eq!(model.limit.output, 64_000);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_combined_returns_none_for_unknown_model() {
        let resolver = MockResolver;
        assert!(resolver.resolve_combined("unknown/model").await.is_none());
    }

    #[test]
    fn split_provider_model_parses_valid_input() {
        assert_eq!(
            split_provider_model("openai/gpt-4o"),
            Some(("openai", "gpt-4o"))
        );
        assert_eq!(
            split_provider_model("anthropic/claude-sonnet-4"),
            Some(("anthropic", "claude-sonnet-4"))
        );
        assert_eq!(
            split_provider_model("zenmux/openai/gpt-5"),
            Some(("zenmux", "openai/gpt-5"))
        );
    }

    #[test]
    fn split_provider_model_returns_none_for_invalid_input() {
        assert_eq!(split_provider_model("no-slash"), None);
        assert_eq!(split_provider_model(""), None);
        assert_eq!(split_provider_model("/"), None);
        assert_eq!(split_provider_model("openai/"), None);
        assert_eq!(split_provider_model("/gpt-4o"), None);
    }
}
