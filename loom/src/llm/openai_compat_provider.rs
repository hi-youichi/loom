use async_trait::async_trait;

use crate::error::AgentError;
use crate::llm::{ChatOpenAICompat, LlmClient, LlmProvider};
use crate::llm::model_registry::{ModelEntry, ProviderConfig};

pub struct OpenAICompatProvider {
    base_url: String,
    api_key: String,
    provider_name: String,
    default_model: String,
    #[allow(dead_code)]
    providers: Vec<ProviderConfig>,
}

impl OpenAICompatProvider {
    pub fn from_entry(entry: &ModelEntry, providers: Vec<ProviderConfig>) -> Self {
        let api_key = entry.api_key.clone().unwrap_or_default();
        let base_url = entry
            .base_url
            .clone()
            .unwrap_or_default();
        Self {
            base_url,
            api_key,
            provider_name: entry.provider.clone(),
            default_model: entry.name.clone(),
            providers,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAICompatProvider {
    fn create_client(&self, model: &str) -> Result<Box<dyn LlmClient>, AgentError> {
        let client =
            ChatOpenAICompat::with_config(self.base_url.clone(), self.api_key.clone(), model);
        Ok(Box::new(client))
    }

    fn create_client_with_headers(
        &self,
        model: &str,
        headers: Option<crate::llm::LlmHeaders>,
    ) -> Result<Box<dyn LlmClient>, AgentError> {
        let mut client =
            ChatOpenAICompat::with_config(self.base_url.clone(), self.api_key.clone(), model);
        if let Some(h) = headers {
            client = client.with_headers(h);
        }
        Ok(Box::new(client))
    }

    fn default_model(&self) -> &str {
        &self.default_model
    }

    fn provider_name(&self) -> &str {
        &self.provider_name
    }
}
