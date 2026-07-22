//! Conversion from `ProviderDef` (config.toml) to `ProviderConfig` (model-spec-core).

use crate::xdg_toml::{FullConfig, ProviderDef};
use model_spec_core::registry::ProviderConfig;

/// Convert a loaded `FullConfig`'s providers into `ProviderConfig` instances.
pub fn load_provider_configs(config: &FullConfig) -> Vec<ProviderConfig> {
    config
        .providers
        .iter()
        .map(provider_def_to_config)
        .collect()
}

/// Convenience: load from XDG config path (`~/.loom/config.toml`).
pub fn load_provider_configs_from_xdg() -> Option<Vec<ProviderConfig>> {
    let config = crate::load_full_config("loom").ok()?;
    Some(load_provider_configs(&config))
}

fn provider_def_to_config(p: &ProviderDef) -> ProviderConfig {
    ProviderConfig {
        name: p.name.clone(),
        base_url: p.base_url.clone(),
        api_key: p.api_key.clone(),
        provider_type: p.provider_type.clone(),
        fetch_models: p.fetch_models.unwrap_or(false),
        cache_ttl: p.cache_ttl,
        enable_tier_resolution: p.enable_tier_resolution.unwrap_or(true),
        declared_models: p.models.iter().map(|m| m.id.clone()).collect(),
    }
}
