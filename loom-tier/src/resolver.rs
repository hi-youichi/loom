use async_trait::async_trait;
use loom_react_config::ReactBuildConfig;
use crate::ModelEntry;
use model_spec_core::spec::ModelTier;
use crate::provider::load_provider_configs;

use crate::resolve_tier_intelligent;

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

#[cfg(test)]
mod tests {
    use super::*;
    use loom_llm::registry::ModelEntry;

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
            tool_choice: None,
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
            tool_choice: None,
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
            tool_choice: None,
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
}

#[async_trait]
pub trait TierResolver: Send + Sync {
    async fn resolve_tier(
        &self,
        config: &ReactBuildConfig,
        tier: ModelTier,
    ) -> Option<ResolvedTierModel>;
}

pub struct DefaultTierResolver;

#[async_trait]
impl TierResolver for DefaultTierResolver {
    async fn resolve_tier(
        &self,
        config: &ReactBuildConfig,
        tier: ModelTier,
    ) -> Option<ResolvedTierModel> {
        let providers = load_provider_configs()?;

        match config.model.as_deref() {
            Some(model_id) => {
                if let Some((provider, _model)) = ModelEntry::parse_id(model_id) {
                    if let Some(provider_cfg) = providers.iter().find(|p| p.name == provider) {
                        if provider_cfg.enable_tier_resolution {
                            let entry =
                                resolve_tier_intelligent(provider, tier, &providers).await?;
                            return Some(ResolvedTierModel::from_entry(entry));
                        }
                    }
                }

                let entry = crate::resolve_for_model(model_id, tier, &providers).await?;
                Some(ResolvedTierModel::from_entry(entry))
            }
            None => {
                let provider = extract_provider_from_config(config);
                match provider {
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
                            source = if config.parent_model_hint.is_some() { "parent_model_hint" } else { "llm_provider" },
                            "Resolving tier from provider"
                        );
                        let entry = resolve_tier_intelligent(&p, tier, &providers).await?;
                        Some(ResolvedTierModel::from_entry(entry))
                    }
                    None => {
                        for p in &providers {
                            if p.enable_tier_resolution {
                                if let Some(entry) = resolve_tier_intelligent(&p.name, tier, &providers).await
                                {
                                    return Some(ResolvedTierModel::from_entry(entry));
                                }
                            }
                        }
                        None
                    }
                }
            }
        }
    }
}

pub async fn resolve_tier_for_config(
    config: &ReactBuildConfig,
    tier: ModelTier,
) -> Option<ResolvedTierModel> {
    DefaultTierResolver.resolve_tier(config, tier).await
}

fn extract_provider_from_config(config: &ReactBuildConfig) -> Option<String> {
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
