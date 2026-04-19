use async_trait::async_trait;

use crate::agent::react::ReactBuildConfig;
use crate::llm::ModelEntry;
use crate::model_spec::ModelTier;
use crate::provider::load_provider_configs;

use super::resolve::resolve_tier_intelligent;

pub struct ResolvedTierModel {
    pub model_id: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub provider_type: Option<String>,
}

impl ResolvedTierModel {
    pub fn from_entry(entry: ModelEntry) -> Self {
        Self {
            model_id: entry.id,
            base_url: entry.base_url,
            api_key: entry.api_key,
            provider_type: entry.provider_type,
        }
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

                let entry = super::resolve::resolve_for_model(model_id, tier, &providers).await?;
                Some(ResolvedTierModel::from_entry(entry))
            }
            None => {
                let provider = config.llm_provider.as_deref();
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

                        let entry = resolve_tier_intelligent(p, tier, &providers).await?;
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

pub(crate) async fn resolve_tier_for_config(
    config: &ReactBuildConfig,
    tier: ModelTier,
) -> Option<ResolvedTierModel> {
    DefaultTierResolver.resolve_tier(config, tier).await
}
