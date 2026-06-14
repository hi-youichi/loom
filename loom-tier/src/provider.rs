//! Provider configuration loading from environment/config.

use loom_llm::registry::ProviderConfig;

/// Load provider configurations from the loom config file.
pub fn load_provider_configs() -> Option<Vec<ProviderConfig>> {
    let config = env_config::load_full_config("loom").ok()?;
    Some(
        config
            .providers
            .into_iter()
            .map(|p| ProviderConfig {
                name: p.name,
                base_url: p.base_url,
                api_key: p.api_key,
                provider_type: p.provider_type,
                fetch_models: p.fetch_models.unwrap_or(false),
                cache_ttl: p.cache_ttl,
                enable_tier_resolution: p.enable_tier_resolution.unwrap_or(true),
                declared_models: p.models.into_iter().map(|m| m.id).collect(),
            })
            .collect(),
    )
}
