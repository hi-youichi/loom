use crate::llm::{ModelEntry, ModelRegistry, ProviderConfig};

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

pub async fn resolve_from_spec(
    provider: &str,
    tier: model_spec_core::spec::ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry> {
    let registry = ModelRegistry::global();
    let (provider_cfg, spec_provider) = registry
        .find_provider_data(provider, providers)
        .await?;

    let (model_id, _model) =
        model_spec_core::spec::pick_best_for_tier(&spec_provider.models, tier)?;

    Some(entry_with_spec_fallback(provider_cfg, model_id, spec_provider.api.as_ref()))
}

pub async fn resolve_from_provider_api(
    provider: &str,
    _tier: model_spec_core::spec::ModelTier,
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

pub async fn resolve_from_local(
    _provider: &str,
    _tier: model_spec_core::spec::ModelTier,
    _providers: &[ProviderConfig],
) -> Option<ModelEntry> {
    None
}

pub async fn resolve_tier_intelligent(
    provider: &str,
    tier: model_spec_core::spec::ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry> {
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

pub async fn resolve_for_model(
    model_id: &str,
    tier: model_spec_core::spec::ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry> {
    let (provider, _) = ModelEntry::parse_id(model_id)?;
    resolve_tier_intelligent(provider, tier, providers).await
}

pub async fn resolve_tier_to_model_id(
    provider: &str,
    tier: model_spec_core::spec::ModelTier,
    providers: &[ProviderConfig],
) -> Option<String> {
    resolve_tier_intelligent(provider, tier, providers).await.map(|e| e.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_resolve_tier_returns_none_for_unknown_provider() {
        let providers = vec![ProviderConfig {
            name: "test_provider".to_string(),
            base_url: Some("https://api.test.com/v1".to_string()),
            api_key: Some("sk-test".to_string()),
            provider_type: None,
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
        }];
        let result = resolve_tier_intelligent(
            "unknown_provider",
            model_spec_core::spec::ModelTier::Light,
            &providers,
        )
        .await;
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_for_model_extracts_provider() {
        let _providers: Vec<ProviderConfig> = vec![];
        let result = ModelEntry::parse_id("openai/gpt-4o");
        assert_eq!(result, Some(("openai", "gpt-4o")));
    }
}
