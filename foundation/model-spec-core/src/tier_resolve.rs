//! Tier resolution: trait, strategies, and intelligent resolution.
//!
//! Merged from `loom-tier/src/resolve.rs` and `loom-tier/src/resolver.rs`.

use async_trait::async_trait;

use crate::registry::{ModelEntry, ProviderConfig};
use crate::{pick_best_for_tier, ModelTier};

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
                            let entry = resolve_tier_intelligent(provider, tier, providers).await?;
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
    let (provider_cfg, spec_provider) = registry.find_provider_data(provider, providers).await?;

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
    let model_list = registry
        .fetch_provider_models_cached(provider_cfg)
        .await
        .ok()?;

    if let Some(first_model) = model_list.first() {
        return Some(first_model.clone());
    }

    None
}

/// Resolve a model from the embedded tier plans.
///
/// Plans are stored with compound keys (`provider_id/family/version`), but the
/// caller passes a config provider name (e.g. `zhipuai-coding-plan`).  We match
/// by checking whether the config name starts with the plan's `provider_id`
/// followed by `-` or `_`, then pick the highest-version matching plan.
pub fn resolve_from_plan(
    provider: &str,
    tier: ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry> {
    let plans = tier_plans();
    let provider_cfg = providers.iter().find(|p| p.name == provider)?;

    let matching: Vec<&crate::tier_plan::TierPlan> = plans
        .values()
        .filter(|p| provider_id_matches(&p.provider_id, provider))
        .collect();

    if matching.is_empty() {
        return None;
    }

    let best = matching.into_iter().max_by(|a, b| {
        compare_versions(
            a.version.as_deref().unwrap_or("0"),
            b.version.as_deref().unwrap_or("0"),
        )
    })?;

    let model_id = best.tiers.get(&tier)?;
    let mut entry = ModelEntry::from_provider_config(provider_cfg, model_id);
    entry.family = best.family.clone();
    entry.version = best.version.clone();
    Some(entry)
}

/// Check if a plan `provider_id` matches a config provider name.
///
/// `zhipuai` matches `zhipuai`, `zhipuai-coding-plan`, `zhipuai_plan`,
/// but NOT `zhipuaiai` (no separator after the prefix).
fn provider_id_matches(plan_id: &str, config_name: &str) -> bool {
    if plan_id == config_name {
        return true;
    }
    let plan_lower = plan_id.to_ascii_lowercase();
    let config_lower = config_name.to_ascii_lowercase();
    config_lower.starts_with(&format!("{plan_lower}-"))
        || config_lower.starts_with(&format!("{plan_lower}_"))
}

/// Compare two dotted version strings (e.g. "5.2" > "5.1" > "5").
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<u32> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let pb: Vec<u32> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    let len = pa.len().max(pb.len());
    for i in 0..len {
        let va = pa.get(i).copied().unwrap_or(0);
        let vb = pb.get(i).copied().unwrap_or(0);
        match va.cmp(&vb) {
            std::cmp::Ordering::Equal => continue,
            ord => return ord,
        }
    }
    std::cmp::Ordering::Equal
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

        assert_eq!(
            result.base_url,
            Some("https://provider.url.com".to_string())
        );
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

    #[test]
    fn test_provider_id_matches_exact() {
        assert!(provider_id_matches("zhipuai", "zhipuai"));
        assert!(provider_id_matches("openai", "openai"));
    }

    #[test]
    fn test_provider_id_matches_dash_suffix() {
        assert!(provider_id_matches("zhipuai", "zhipuai-coding-plan"));
        assert!(provider_id_matches("deepseek", "deepseek-cn"));
    }

    #[test]
    fn test_provider_id_matches_underscore_suffix() {
        assert!(provider_id_matches("zhipuai", "zhipuai_plan"));
    }

    #[test]
    fn test_provider_id_matches_no_separator() {
        assert!(!provider_id_matches("zhipuai", "zhipuaiai"));
        assert!(!provider_id_matches("deep", "deepseek"));
    }

    #[test]
    fn test_provider_id_matches_case_insensitive() {
        assert!(provider_id_matches("ZhipuAI", "zhipuai-coding-plan"));
        assert!(provider_id_matches("zhipuai", "ZHIPUAI-Coding-Plan"));
    }

    #[test]
    fn test_provider_id_matches_unrelated() {
        assert!(!provider_id_matches("zhipuai", "openai"));
        assert!(!provider_id_matches("zhipuai", "anthropic"));
    }

    #[test]
    fn test_compare_versions_basic() {
        use std::cmp::Ordering;
        assert_eq!(compare_versions("5.2", "5.1"), Ordering::Greater);
        assert_eq!(compare_versions("5.1", "5.2"), Ordering::Less);
        assert_eq!(compare_versions("5", "5"), Ordering::Equal);
        assert_eq!(compare_versions("5.2", "5"), Ordering::Greater);
        assert_eq!(compare_versions("5", "5.2"), Ordering::Less);
    }

    #[test]
    fn test_compare_versions_three_parts() {
        use std::cmp::Ordering;
        assert_eq!(compare_versions("5.2.1", "5.2.0"), Ordering::Greater);
        assert_eq!(compare_versions("5.2.0", "5.2.1"), Ordering::Less);
        assert_eq!(compare_versions("5.2.1", "5.2"), Ordering::Greater);
    }

    #[test]
    fn test_resolve_from_plan_prefix_match() {
        let provider = ProviderConfig {
            name: "zhipuai-coding-plan".to_string(),
            base_url: Some("https://api.modelgate.dev/v1".to_string()),
            api_key: Some("test_key".to_string()),
            provider_type: Some("openai_compat".to_string()),
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
            declared_models: Vec::new(),
        };
        let providers = vec![provider];

        let entry = resolve_from_plan("zhipuai-coding-plan", ModelTier::Light, &providers);
        assert!(entry.is_some(), "Should match zhipuai plan via prefix");
        let entry = entry.unwrap();
        assert_eq!(
            entry.name, "glm-4.7",
            "Version 5.2 plan should be selected (highest)"
        );
        assert_eq!(entry.family.as_deref(), Some("glm"));
        assert_eq!(entry.version.as_deref(), Some("5.2"));
    }

    #[test]
    fn test_resolve_from_plan_no_match() {
        let provider = ProviderConfig {
            name: "unknown-provider".to_string(),
            base_url: Some("https://api.unknown.com".to_string()),
            api_key: Some("key".to_string()),
            provider_type: None,
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
            declared_models: Vec::new(),
        };
        let providers = vec![provider];

        let entry = resolve_from_plan("unknown-provider", ModelTier::Light, &providers);
        assert!(entry.is_none(), "Should not match any plan");
    }

    #[test]
    fn test_resolve_from_plan_strong_tier() {
        let provider = ProviderConfig {
            name: "zhipuai".to_string(),
            base_url: Some("https://api.test.com".to_string()),
            api_key: Some("key".to_string()),
            provider_type: None,
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
            declared_models: Vec::new(),
        };
        let providers = vec![provider];

        let entry = resolve_from_plan("zhipuai", ModelTier::Strong, &providers);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().name, "glm-5.2");
    }
}
