//! Tier resolution strategies: plan, spec, provider API, local.

use model_spec_core::registry::{ModelEntry, ProviderConfig};

use crate::model_registry::ModelRegistry;
use crate::plan::tier_plans;

fn entry_with_spec_fallback(
    provider_cfg: &ProviderConfig,
    model_id: &str,
    spec_api: Option<&String>,
) -> ModelEntry {
    let mut entry = ModelEntry::from_provider_config(provider_cfg, model_id);
    if entry.base_url.is_none() {
        if let Some(api) = spec_api {
            entry.base_url = Some(api.clone());
        }
    }
    if entry.provider_type.is_none() && !entry.provider.eq_ignore_ascii_case("openai") {
        entry.provider_type = Some("openai_compat".to_string());
    }
    entry
}

/// Resolve a model from the model spec (models.dev) for the given tier.
pub async fn resolve_from_spec(
    provider: &str,
    tier: model_spec_core::ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry> {
    let registry = ModelRegistry::global();
    let (provider_cfg, spec_provider) = registry
        .find_provider_data(provider, providers)
        .await?;

    let (model_id, _model) =
        model_spec_core::pick_best_for_tier(&spec_provider.models, tier)?;

    Some(entry_with_spec_fallback(provider_cfg, model_id, spec_provider.api.as_ref()))
}

/// Resolve a model by fetching from the provider's API.
pub async fn resolve_from_provider_api(
    provider: &str,
    _tier: model_spec_core::ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry> {
    let provider_cfg = providers.iter().find(|p| p.name == provider)?;

    if !provider_cfg.fetch_models {
        return None;
    }

    let registry = ModelRegistry::global();
    let model_list = registry.fetch_provider_models_cached(provider_cfg).await.ok()?;

    if let Some(first_model) = model_list.first() {
        return Some(first_model.clone());
    }

    None
}

/// Resolve a model from local storage (not yet implemented).
pub async fn resolve_from_local(
    _provider: &str,
    _tier: model_spec_core::ModelTier,
    _providers: &[ProviderConfig],
) -> Option<ModelEntry> {
    None
}

/// Resolve a model from the embedded tier plans.
pub fn resolve_from_plan(
    provider: &str,
    tier: model_spec_core::ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry> {
    let plans = tier_plans();
    let plan = plans.get(provider)?;
    let model_id = plan.tiers.get(&tier)?;
    let provider_cfg = providers.iter().find(|p| p.name == provider)?;
    let mut entry = ModelEntry::from_provider_config(provider_cfg, model_id);
    entry.family = plan.family.clone();
    entry.version = plan.version.clone();
    Some(entry)
}

/// Resolve a tier using all available strategies (plan → spec → provider API → local).
pub async fn resolve_tier_intelligent(
    provider: &str,
    tier: model_spec_core::ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry> {
    if let Some(entry) = resolve_from_plan(provider, tier, providers) {
        tracing::debug!(
            provider = %provider,
            tier = ?tier,
            "Tier resolution succeeded using plan"
        );
        return Some(entry);
    }

    if let Some(entry) = resolve_from_spec(provider, tier, providers).await {
        tracing::debug!(
            provider = %provider,
            tier = ?tier,
            "Tier resolution succeeded using models.dev"
        );
        return Some(entry);
    }

    if let Some(entry) = resolve_from_provider_api(provider, tier, providers).await {
        tracing::debug!(
            provider = %provider,
            tier = ?tier,
            "Tier resolution succeeded using provider API"
        );
        return Some(entry);
    }

    if let Some(entry) = resolve_from_local(provider, tier, providers).await {
        tracing::debug!(
            provider = %provider,
            tier = ?tier,
            "Tier resolution succeeded using local models"
        );
        return Some(entry);
    }

    tracing::warn!(
        provider = %provider,
        tier = ?tier,
        "Tier resolution failed using all methods"
    );
    None
}

/// Resolve a tier for a specific model ID (extracts provider from the ID).
pub async fn resolve_for_model(
    model_id: &str,
    tier: model_spec_core::ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry> {
    let (provider, _) = ModelEntry::parse_id(model_id)?;
    resolve_tier_intelligent(provider, tier, providers).await
}

/// Resolve a tier and return just the model ID string.
pub async fn resolve_tier_to_model_id(
    provider: &str,
    tier: model_spec_core::ModelTier,
    providers: &[ProviderConfig],
) -> Option<String> {
    resolve_tier_intelligent(provider, tier, providers).await.map(|e| e.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_with_spec_fallback_base_url_missing() {
        let provider_cfg = ProviderConfig {
            name: "test_provider".to_string(),
            base_url: None,
            api_key: Some("test_key".to_string()),
            provider_type: Some("openai".to_string()),
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
            declared_models: Vec::new(),
        };

        let spec_api = Some("https://spec.api.com".to_string());
        let result = entry_with_spec_fallback(&provider_cfg, "test_model", spec_api.as_ref());

        assert_eq!(result.id, "test_provider/test_model");
        assert_eq!(result.base_url, Some("https://spec.api.com".to_string()));
        assert_eq!(result.provider, "test_provider");
    }

    #[test]
    fn test_entry_with_spec_fallback_base_url_present() {
        let provider_cfg = ProviderConfig {
            name: "test_provider".to_string(),
            base_url: Some("https://provider.url.com".to_string()),
            api_key: Some("provider_key".to_string()),
            provider_type: Some("openai".to_string()),
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
            declared_models: Vec::new(),
        };

        let spec_api = Some("https://spec.api.com".to_string());
        let result = entry_with_spec_fallback(&provider_cfg, "test_model", spec_api.as_ref());

        assert_eq!(result.base_url, Some("https://provider.url.com".to_string()));
    }

    #[test]
    fn test_entry_with_spec_fallback_provider_type_openai() {
        let provider_cfg = ProviderConfig {
            name: "openai".to_string(),
            base_url: Some("https://api.openai.com".to_string()),
            api_key: Some("openai_key".to_string()),
            provider_type: None,
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
            declared_models: Vec::new(),
        };

        let result = entry_with_spec_fallback(&provider_cfg, "gpt-4", None);
        assert_eq!(result.provider_type, None);
    }

    #[test]
    fn test_entry_with_spec_fallback_provider_type_non_openai() {
        let provider_cfg = ProviderConfig {
            name: "custom_provider".to_string(),
            base_url: Some("https://custom.api.com".to_string()),
            api_key: Some("custom_key".to_string()),
            provider_type: None,
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
            declared_models: Vec::new(),
        };

        let result = entry_with_spec_fallback(&provider_cfg, "custom_model", None);
        assert_eq!(result.provider_type, Some("openai_compat".to_string()));
    }

    #[test]
    fn test_entry_with_spec_fallback_case_insensitive() {
        let provider_cfg = ProviderConfig {
            name: "OPENAI".to_string(),
            base_url: Some("https://api.openai.com".to_string()),
            api_key: Some("key".to_string()),
            provider_type: None,
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
            declared_models: Vec::new(),
        };

        let result = entry_with_spec_fallback(&provider_cfg, "gpt-4", None);
        assert_eq!(result.provider_type, None);
    }

    #[test]
    fn test_entry_with_spec_fallback_no_spec_api() {
        let provider_cfg = ProviderConfig {
            name: "test_provider".to_string(),
            base_url: None,
            api_key: Some("key".to_string()),
            provider_type: None,
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
            declared_models: Vec::new(),
        };

        let result = entry_with_spec_fallback(&provider_cfg, "model", None);
        assert_eq!(result.base_url, None);
        assert_eq!(result.provider_type, Some("openai_compat".to_string()));
    }

    #[test]
    fn test_entry_with_spec_fallback_complete_case() {
        let provider_cfg = ProviderConfig {
            name: "complete_provider".to_string(),
            base_url: Some("https://complete.com".to_string()),
            api_key: Some("complete_key".to_string()),
            provider_type: Some("custom_type".to_string()),
            fetch_models: true,
            cache_ttl: Some(3600),
            enable_tier_resolution: true,
            declared_models: Vec::new(),
        };

        let spec_api = Some("https://spec.com".to_string());
        let result = entry_with_spec_fallback(&provider_cfg, "complete_model", spec_api.as_ref());

        assert_eq!(result.id, "complete_provider/complete_model");
        assert_eq!(result.provider, "complete_provider");
        assert_eq!(result.base_url, Some("https://complete.com".to_string()));
        assert_eq!(result.api_key, Some("complete_key".to_string()));
        assert_eq!(result.provider_type, Some("custom_type".to_string()));
    }

    #[tokio::test]
    async fn test_resolve_from_local_always_none() {
        let result = resolve_from_local("test", model_spec_core::ModelTier::Light, &[]).await;
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_for_model_extracts_provider() {
        let result = ModelEntry::parse_id("openai/gpt-4o");
        assert_eq!(result, Some(("openai", "gpt-4o")));
    }

    #[test]
    fn test_model_entry_parse_id_invalid_format() {
        let result = ModelEntry::parse_id("invalid_id_without_slash");
        assert_eq!(result, None);
    }

    #[test]
    fn test_model_entry_parse_id_empty_provider() {
        let result = ModelEntry::parse_id("/model_only");
        assert_eq!(result, Some(("", "model_only")));
    }

    #[test]
    fn test_model_entry_parse_id_empty_model() {
        let result = ModelEntry::parse_id("provider/");
        assert_eq!(result, Some(("provider", "")));
    }
}