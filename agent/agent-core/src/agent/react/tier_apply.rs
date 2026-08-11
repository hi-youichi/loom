//! Apply tier resolution results to [`ReactBuildConfig`].
//!
//! Migrated from `loom-tier/src/apply.rs`. Lives in `loom-react-config` because
//! it reads and writes `ReactBuildConfig` fields.

use model_spec_core::{registry::ModelEntry, DefaultTierResolver, TierResolver};

use crate::agent::ReactBuildConfig;

/// Resolve `model_tier` in the config using the default resolver, returning a new config.
pub async fn resolve_tier_and_build_config(config: &ReactBuildConfig) -> ReactBuildConfig {
    resolve_tier_and_build_config_with_resolver(config, &DefaultTierResolver).await
}

/// Resolve `model_tier` in the config using a custom resolver, returning a new config.
pub async fn resolve_tier_and_build_config_with_resolver(
    config: &ReactBuildConfig,
    resolver: &dyn TierResolver,
) -> ReactBuildConfig {
    let Some(tier) = config.model_tier else {
        tracing::debug!("No model_tier set, returning config as-is");
        return config.clone();
    };
    let mut config = config.clone();
    tracing::info!("Resolving model tier: {:?}", tier);

    let providers = env_config::load_provider_configs_from_xdg().unwrap_or_default();
    let provider_hint = extract_provider_hint(&config);

    match resolver
        .resolve_tier(
            config.model.as_deref(),
            tier,
            provider_hint.as_deref(),
            &providers,
        )
        .await
    {
        Some(resolved) => {
            tracing::info!(
                tier = ?tier,
                resolved_model = %resolved.model_id,
                resolved_provider = ?resolved.provider_type,
                resolved_base_url = ?resolved.base_url,
                "Tier resolution successful, applying complete model configuration"
            );

            config.model = Some(resolved.model_id);
            config.model_tier = None;

            if let Some(base_url) = resolved.base_url {
                tracing::debug!(base_url = %base_url, "Applying base_url from tier resolution");
                config.openai_base_url = Some(base_url);
            }
            if let Some(api_key) = resolved.api_key {
                tracing::debug!(
                    "Applying api_key from tier resolution (length: {})",
                    api_key.len()
                );
                config.openai_api_key = Some(api_key);
            }
            if let Some(provider_type) = resolved.provider_type {
                tracing::debug!(
                    provider_type = %provider_type,
                    "Applying provider_type from tier resolution"
                );
                config.llm_provider = Some(provider_type);
            }
            if let Some(provider_name) = resolved.provider_name {
                tracing::debug!(
                    provider_name = %provider_name,
                    "Applying provider_name from tier resolution"
                );
                config.llm_provider_name = Some(provider_name);
            }

            config
        }
        None => {
            tracing::warn!(tier = ?tier, "Tier resolution failed, returning config as-is");
            config
        }
    }
}

/// Extract provider hint from [`ReactBuildConfig`] fields.
///
/// Priority: `parent_model_hint` (parsed as `provider/model`) → `llm_provider_name` → `llm_provider`.
pub fn extract_provider_hint(config: &ReactBuildConfig) -> Option<String> {
    if let Some(hint) = config.parent_model_hint.as_deref() {
        if let Some((provider, _)) = ModelEntry::parse_id(hint) {
            return Some(provider.to_string());
        }
    }
    config
        .llm_provider_name
        .as_deref()
        .or(config.llm_provider.as_deref())
        .map(|p| p.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use model_spec_core::{ModelTier, ProviderConfig, ResolvedTierModel};

    struct MockTierResolver {
        should_resolve: bool,
        test_result: Option<ResolvedTierModel>,
    }

    #[async_trait::async_trait]
    impl TierResolver for MockTierResolver {
        async fn resolve_tier(
            &self,
            _model: Option<&str>,
            _tier: ModelTier,
            _provider_hint: Option<&str>,
            _providers: &[ProviderConfig],
        ) -> Option<ResolvedTierModel> {
            if self.should_resolve {
                self.test_result.clone()
            } else {
                None
            }
        }
    }

    #[tokio::test]
    async fn test_resolve_tier_and_build_config_no_tier() {
        let config = ReactBuildConfig {
            model_tier: None,
            model: Some("test_model".to_string()),
            ..Default::default()
        };

        let resolver = MockTierResolver {
            should_resolve: true,
            test_result: Some(ResolvedTierModel {
                model_id: "resolved_model".to_string(),
                base_url: Some("https://resolved.com".to_string()),
                api_key: Some("resolved_key".to_string()),
                provider_type: Some("resolved_type".to_string()),
                provider_name: Some("resolved_provider".to_string()),
            }),
        };

        let result = resolve_tier_and_build_config_with_resolver(&config, &resolver).await;

        assert_eq!(result.model_tier, None);
        assert_eq!(result.model.as_deref(), Some("test_model"));
        assert_eq!(result.openai_base_url, None);
        assert_eq!(result.openai_api_key, None);
    }

    #[tokio::test]
    async fn test_resolve_tier_and_build_config_resolution_success() {
        let config = ReactBuildConfig {
            model_tier: Some(ModelTier::Strong),
            model: None,
            ..Default::default()
        };

        let resolver = MockTierResolver {
            should_resolve: true,
            test_result: Some(ResolvedTierModel {
                model_id: "resolved_model".to_string(),
                base_url: Some("https://resolved.com".to_string()),
                api_key: Some("resolved_key".to_string()),
                provider_type: Some("resolved_type".to_string()),
                provider_name: Some("resolved_provider".to_string()),
            }),
        };

        let result = resolve_tier_and_build_config_with_resolver(&config, &resolver).await;

        assert_eq!(result.model, Some("resolved_model".to_string()));
        assert_eq!(
            result.openai_base_url,
            Some("https://resolved.com".to_string())
        );
        assert_eq!(result.openai_api_key, Some("resolved_key".to_string()));
        assert_eq!(result.llm_provider, Some("resolved_type".to_string()));
        assert_eq!(
            result.llm_provider_name,
            Some("resolved_provider".to_string())
        );
    }

    #[tokio::test]
    async fn test_resolve_tier_and_build_config_resolution_failure() {
        let config = ReactBuildConfig {
            model_tier: Some(ModelTier::Strong),
            model: Some("original_model".to_string()),
            openai_base_url: Some("https://original.com".to_string()),
            openai_api_key: Some("original_key".to_string()),
            llm_provider: Some("original_type".to_string()),
            llm_provider_name: Some("original_provider".to_string()),
            ..Default::default()
        };

        let resolver = MockTierResolver {
            should_resolve: false,
            test_result: None,
        };

        let result = resolve_tier_and_build_config_with_resolver(&config, &resolver).await;

        assert_eq!(result.model, Some("original_model".to_string()));
        assert_eq!(
            result.openai_base_url,
            Some("https://original.com".to_string())
        );
        assert_eq!(result.openai_api_key, Some("original_key".to_string()));
        assert_eq!(result.llm_provider, Some("original_type".to_string()));
        assert_eq!(
            result.llm_provider_name,
            Some("original_provider".to_string())
        );
    }

    #[tokio::test]
    async fn test_resolve_tier_and_build_config_partial_resolution() {
        let config = ReactBuildConfig {
            model_tier: Some(ModelTier::Standard),
            model: None,
            ..Default::default()
        };

        let resolver = MockTierResolver {
            should_resolve: true,
            test_result: Some(ResolvedTierModel {
                model_id: "partial_model".to_string(),
                base_url: Some("https://partial.com".to_string()),
                api_key: None,
                provider_type: None,
                provider_name: None,
            }),
        };

        let result = resolve_tier_and_build_config_with_resolver(&config, &resolver).await;

        assert_eq!(result.model, Some("partial_model".to_string()));
        assert_eq!(
            result.openai_base_url,
            Some("https://partial.com".to_string())
        );
        assert_eq!(result.openai_api_key, None);
        assert_eq!(result.llm_provider, None);
        assert_eq!(result.llm_provider_name, None);
    }
}
