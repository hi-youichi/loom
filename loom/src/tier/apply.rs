use crate::agent::react::ReactBuildConfig;
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
                "resolved model tier successfully"
            );
            config.model = Some(resolved.model_id);
            if config.openai_base_url.is_none() {
                config.openai_base_url = resolved.base_url;
            }
            if config.openai_api_key.is_none() {
                config.openai_api_key = resolved.api_key;
            }
            if config.llm_provider.is_none() && resolved.provider_type.is_some() {
                config.llm_provider = resolved.provider_type;
            }
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
    config
}
