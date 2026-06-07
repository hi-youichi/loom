use loom_react_config::ReactBuildConfig;
use super::resolver::{DefaultTierResolver, TierResolver};

pub async fn resolve_tier_and_build_config(config: &ReactBuildConfig) -> ReactBuildConfig {
    resolve_tier_and_build_config_with_resolver(config, &DefaultTierResolver).await
}

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
    match resolver.resolve_tier(&config, tier).await {
        Some(resolved) => {
            tracing::info!(
                tier = ?tier,
                resolved_model = %resolved.model_id,
                resolved_provider = ?resolved.provider_type,
                resolved_base_url = ?resolved.base_url,
                "Tier resolution successful, applying complete model configuration"
            );

            config.model = Some(resolved.model_id);

            if let Some(base_url) = resolved.base_url {
                tracing::debug!(
                    base_url = %base_url,
                    "Applying base_url from tier resolution"
                );
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

            tracing::debug!(
                final_model = ?config.model,
                final_provider = ?config.llm_provider,
                final_base_url = ?config.openai_base_url,
                has_api_key = config.openai_api_key.is_some(),
                "Applied complete tier-resolved model configuration"
            );
        }
        None => {
            tracing::warn!(
                tier = ?tier,
                model = ?config.model,
                llm_provider = ?config.llm_provider,
                "tier resolution failed, using model as-is"
            );
        }
    }
    config.model_tier = None;
    config.parent_model_hint = None;
    config
}
