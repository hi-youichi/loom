use async_trait::async_trait;
use async_openai::config::OpenAIConfig;

use crate::error::AgentError;
use crate::llm::{ChatOpenAI, LlmClient, LlmProvider};
use crate::llm::model_registry::{ModelEntry, ModelRegistry, ProviderConfig};
use crate::model_spec::ModelTier;

pub struct OpenAIProvider {
    config: OpenAIConfig,
    provider_name: String,
    default_model: String,
    providers: Vec<ProviderConfig>,
}

impl OpenAIProvider {
    pub fn from_entry(entry: &ModelEntry, providers: Vec<ProviderConfig>) -> Self {
        let mut config = OpenAIConfig::new();
        if let Some(ref api_key) = entry.api_key {
            config = config.with_api_key(api_key);
        }
        if let Some(ref base_url) = entry.base_url {
            let base_url = base_url.trim_end_matches('/');
            config = config.with_api_base(base_url);
        }
        Self {
            config,
            provider_name: entry.provider.clone(),
            default_model: entry.name.clone(),
            providers,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    fn create_client(&self, model: &str) -> Result<Box<dyn LlmClient>, AgentError> {
        let client = ChatOpenAI::with_config(self.config.clone(), model);
        Ok(Box::new(client))
    }

    fn default_model(&self) -> &str {
        &self.default_model
    }

    fn provider_name(&self) -> &str {
        &self.provider_name
    }

    async fn resolve_tier(&self, tier: ModelTier) -> Result<String, AgentError> {
        if tier == ModelTier::None {
            return Ok(self.default_model().to_string());
        }
        let entry = ModelRegistry::global()
            .resolve_tier_intelligent(&self.provider_name, tier, &self.providers)
            .await
            .ok_or_else(|| {
                AgentError::ExecutionFailed(format!(
                    "no model found for tier {:?} on provider '{}'",
                    tier, self.provider_name
                ))
            })?;
        Ok(entry.id)
    }
}
