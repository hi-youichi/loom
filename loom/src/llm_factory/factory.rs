//! LLM client factory that unifies provider config loading, tier resolution, and client creation.
//!
//! Usage:
//! ```ignore
//! let factory = LlmFactory::load()?;
//! let entry = factory.resolve_tier("zhipuai", "glm", "5", ModelTier::Strong).await?;
//! let client = factory.build_client(&entry)?;
//! ```

use model_spec_core::ModelTier;

use loom_llm::factory::create_llm_client;
use model_spec_core::registry::{ModelEntry, ProviderConfig};
use loom_graph_core::GraphError;
use loom_llm::traits::LlmClient;

/// LLM client factory that unifies provider config loading, tier resolution, and client creation.
pub struct LlmFactory {
    providers: Vec<ProviderConfig>,
}

impl LlmFactory {
    /// Load provider configs from the config file.
    pub fn load() -> Option<Self> {
        env_config::load_provider_configs_from_xdg().map(|providers| Self { providers })
    }

    /// Resolve a tier to a fully configured ModelEntry.
    ///
    /// Looks up the tier plan by (provider, family, version), finds the model name
    /// for the given tier, and fills in base_url/api_key from the provider config.
    pub async fn resolve_tier(
        &self,
        provider: &str,
        family: &str,
        version: &str,
        tier: ModelTier,
    ) -> Option<ModelEntry> {
        let plans = model_spec_core::tier_plans();
        let key = format!("{}/{}/{}", provider, family, version);
        let plan = plans.get(&key)?;
        let model_name = plan.tiers.get(&tier)?;
        let provider_cfg = self.providers.iter().find(|p| p.name == provider)?;
        let mut entry = ModelEntry::from_provider_config(
            provider_cfg,
            model_name,
        );
        entry.family = Some(family.to_string());
        entry.version = Some(version.to_string());
        Some(entry)
    }

    /// Resolve a different tier from an existing ModelEntry.
    ///
    /// Uses the entry's provider, family, and version (if available) to look up
    /// a new model name for the target tier.
    pub async fn resolve_tier_from_entry(
        &self,
        entry: &ModelEntry,
        tier: ModelTier,
    ) -> Option<ModelEntry> {
        let provider = &entry.provider;
        let family = entry.family.as_deref()?;
        let version = entry.version.as_deref()?;
        self.resolve_tier(provider, family, version, tier).await
    }

    /// Build an LLM client from a ModelEntry.
    pub fn build_client(&self, entry: &ModelEntry) -> Result<Box<dyn LlmClient>, GraphError> {
        create_llm_client(entry, None)
    }
}
