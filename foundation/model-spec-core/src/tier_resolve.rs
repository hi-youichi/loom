//! Tier resolution: trait, strategies, and intelligent resolution.
//!
//! Merged from `loom-tier/src/resolve.rs` and `loom-tier/src/resolver.rs`.

use async_trait::async_trait;

use crate::registry::{ModelEntry, ProviderConfig};
use crate::{ModelTier, pick_best_for_tier};

use crate::model_registry::ModelRegistry;
use crate::tier_plan::tier_plans;

// ============================================================================
// ResolvedTierModel
// ============================================================================

/// The result of tier resolution — a fully resolved model with provider info.
#[derive(Clone)]
pub struct ResolvedTierModel {
    pub model_id: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub provider_type: Option<String>,
    pub provider_name: Option<String>,
}

impl ResolvedTierModel {
    pub fn from_entry(entry: ModelEntry) -> Self {
        Self {
            model_id: entry.id,
            base_url: entry.base_url,
            api_key: entry.api_key,
            provider_type: entry.provider_type,
            provider_name: Some(entry.provider),
        }
    }
}

// ============================================================================
// TierResolver trait + DefaultTierResolver
// ============================================================================

/// Resolves a model tier to a concrete model.
///
/// Takes raw parameters instead of `ReactBuildConfig` to avoid a dependency on
/// `loom-react-config`. Callers extract the provider hint and load providers
/// before calling this trait.
#[async_trait]
pub trait TierResolver: Send + Sync {
    /// Resolve a tier to a concrete model.
    ///
    /// - `model`: explicit model override (e.g. `"openai/gpt-4o"`)
    /// - `tier`: the target tier to resolve
    /// - `provider_hint`: provider name extracted from parent config
    /// - `providers`: loaded provider configurations
    async fn resolve_tier(
        &self,
        model: Option<&str>,
        tier: ModelTier,
        provider_hint: Option<&str>,
        providers: &[ProviderConfig],
    ) -> Option<ResolvedTierModel>;
}

/// Default resolver using plan → spec → provider API → local strategy chain.
pub struct DefaultTierResolver;

#[async_trait]
impl TierResolver for DefaultTierResolver {
    async fn resolve_tier(
        &self,
        model: Option<&str>,
        tier: ModelTier,
        provider_hint: Option<&str>,
        providers: &[ProviderConfig],
    ) -> Option<ResolvedTierModel> {
        match model {
            Some(model_id) => {
                if let Some((provider, _model)) = ModelEntry::parse_id(model_id) {
                    if let Some(provider_cfg) = providers.iter().find(|p| p.name == provider) {
                        if provider_cfg.enable_tier_resolution {
                            let entry =
                                resolve_tier_intelligent(provider, tier, providers).await?;
                            return Some(ResolvedTierModel::from_entry(entry));
                        }
                    }
                }

                let entry = resolve_for_model(model_id, tier, providers).await?;
                Some(ResolvedTierModel::from_entry(entry))
            }
            None => match provider_hint {
                Some(p) => {
                    if let Some(provider_cfg) = providers.iter().find(|cfg| cfg.name == p) {
                        if !provider_cfg.enable_tier_resolution {
                            tracing::debug!(
                                provider = %p,
                                "Tier resolution disabled for this provider"
                            );
                            return None;
                        }
                    }

                    tracing::debug!(
                        provider = %p,
                        ?tier,
                        "Resolving tier from provider"
                    );
                    let entry = resolve_tier_intelligent(p, tier, providers).await?;
                    Some(ResolvedTierModel::from_entry(entry))
                }
                None => {
                    for p in providers {
                        if p.enable_tier_resolution {
                            if let Some(entry) =
                                resolve_tier_intelligent(&p.name, tier, providers).await
                            {
                                return Some(ResolvedTierModel::from_entry(entry));
                            }
                        }
                    }
                    None
                }
            },
        }
    }
}

/// Convenience wrapper: resolve a tier using the default resolver.
pub async fn resolve_tier(
    model: Option<&str>,
    tier: ModelTier,
    provider_hint: Option<&str>,
    providers: &[ProviderConfig],
) -> Option<ResolvedTierModel> {
    DefaultTierResolver
        .resolve_tier(model, tier, provider_hint, providers)
        .await
}

// ============================================================================
// Resolution strategies
// ============================================================================

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
    tier: ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry> {
    let registry = ModelRegistry::global();
    let (provider_cfg, spec_provider) = registry
        .find_provider_data(provider, providers)
        .await?;

    let (model_id, _model) = pick_best_for_tier(&spec_provider.models, tier)?;

    Some(entry_with_spec_fallback(
        provider_cfg,
        model_id,
        spec_provider.api.as_ref(),
    ))
}

/// Resolve a model by fetching from the provider's API.
pub(crate) async fn resolve_from_provider_api(
    provider: &str,
    _tier: ModelTier,
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

/// Resolve a model from the embedded tier plans.
pub fn resolve_from_plan(
    provider: &str,
    tier: ModelTier,
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
    tier: ModelTier,
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

    tracing::warn!(
        provider = %provider,
        tier = ?tier,
        "Tier resolution failed using all methods"
    );
    None
}

/// Resolve a tier for a specific model ID (extracts provider from the ID).
pub(crate) async fn resolve_for_model(
    model_id: &str,
    tier: ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry> {
    let (provider, _) = ModelEntry::parse_id(model_id)?;
    resolve_tier_intelligent(provider, tier, providers).await
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolved_tier_model_from_entry_complete() {
        let entry = ModelEntry {
            id: "test_provider/test_model".to_string(),
            name: "test_model".to_string(),
            provider: "test_provider".to_string(),
            base_url: Some("https://api.test.com".to_string()),
            api_key: Some("test_key".to_string()),
            provider_type: Some("openai_compat".to_string()),
            temperature: None,
            family: Some("test_family".to_string()),
            version: None,
            max_tokens: Some(2048),
        };

        let resolved = ResolvedTierModel::from_entry(entry);
        assert_eq!(resolved.model_id, "test_provider/test_model");
        assert_eq!(resolved.base_url, Some("https://api.test.com".to_string()));
        assert_eq!(resolved.api_key, Some("test_key".to_string()));
        assert_eq!(resolved.provider_type, Some("openai_compat".to_string()));
        assert_eq!(resolved.provider_name, Some("test_provider".to_string()));
    }

    #[test]
    fn test_resolved_tier_model_from_entry_minimal() {
        let entry = ModelEntry {
            id: "provider/model".to_string(),
            name: "model".to_string(),
            provider: "provider".to_string(),
            base_url: None,
            api_key: None,
            provider_type: None,
            temperature: None,
            family: None,
            version: None,
            max_tokens: None,
        };

        let resolved = ResolvedTierModel::from_entry(entry);
        assert_eq!(resolved.model_id, "provider/model");
        assert_eq!(resolved.base_url, None);
        assert_eq!(resolved.api_key, None);
        assert_eq!(resolved.provider_type, None);
        assert_eq!(resolved.provider_name, Some("provider".to_string()));
    }

    #[test]
    fn test_resolved_tier_model_partial_fields() {
        let entry = ModelEntry {
            id: "prov/model".to_string(),
            name: "model".to_string(),
            provider: "prov".to_string(),
            base_url: Some("https://url.com".to_string()),
            api_key: None,
            provider_type: Some("custom".to_string()),
            temperature: None,
            family: None,
            version: None,
            max_tokens: None,
        };

        let resolved = ResolvedTierModel::from_entry(entry);
        assert_eq!(resolved.model_id, "prov/model");
        assert_eq!(resolved.base_url, Some("https://url.com".to_string()));
        assert_eq!(resolved.api_key, None);
        assert_eq!(resolved.provider_type, Some("custom".to_string()));
        assert_eq!(resolved.provider_name, Some("prov".to_string()));
    }

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
