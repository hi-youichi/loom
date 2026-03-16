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

    /// Resolve model spec from a combined string "provider/model".
    ///
    /// # Examples
    /// - `"openai/gpt-4o"` -> provider="openai", model="gpt-4o"
    /// - `"anthropic/claude-sonnet-4"` -> provider="anthropic", model="claude-sonnet-4"
    /// - `"google/gemini-2.5-pro"` -> provider="google", model="gemini-2.5-pro"
    ///
    /// Returns `None` if the string doesn't contain '/' or model not found.
    async fn resolve_combined(&self, model: &str) -> Option<ModelSpec> {
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

#[cfg(test)]
mod tests {
    use super::*;

    struct MockResolver;

    #[async_trait]
    impl ModelLimitResolver for MockResolver {
        async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<ModelSpec> {
            if provider_id == "openai" && model_id == "gpt-4o" {
                Some(ModelSpec::new(128_000, 16_384))
            } else if provider_id == "zenmux" && model_id == "openai/gpt-5" {
                Some(ModelSpec::new(400_000, 64_000))
            } else {
                None
            }
        }
    }

    #[tokio::test]
    async fn resolve_combined_splits_provider_and_model() {
        let resolver = MockResolver;
        let spec = resolver.resolve_combined("openai/gpt-4o").await.unwrap();
        assert_eq!(spec.context_limit, 128_000);
        assert_eq!(spec.output_limit, 16_384);
    }

    #[tokio::test]
    async fn resolve_combined_handles_nested_model_id() {
        let resolver = MockResolver;
        // "zenmux/openai/gpt-5" -> provider="zenmux", model="openai/gpt-5"
        let spec = resolver.resolve_combined("zenmux/openai/gpt-5").await.unwrap();
        assert_eq!(spec.context_limit, 400_000);
        assert_eq!(spec.output_limit, 64_000);
    }

    #[tokio::test]
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
