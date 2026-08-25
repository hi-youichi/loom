//! OpenAI LlmProvider implementation.

use async_openai::config::OpenAIConfig;
use async_trait::async_trait;

use crate::client::ChatOpenAI;
use crate::registry::ModelEntry;
use crate::traits::{LlmClient, LlmHeaders, LlmProvider};
use anureo_graph_core::GraphError;

pub struct OpenAIProvider {
    config: OpenAIConfig,
    provider_name: String,
    default_model: String,
}

impl OpenAIProvider {
    pub fn from_entry(entry: &ModelEntry) -> Self {
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
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    fn create_client(&self, model: &str) -> Result<Box<dyn LlmClient>, GraphError> {
        let client = ChatOpenAI::with_config(self.config.clone(), model);
        Ok(Box::new(client))
    }

    fn create_client_with_headers(
        &self,
        model: &str,
        headers: Option<LlmHeaders>,
    ) -> Result<Box<dyn LlmClient>, GraphError> {
        let mut client = ChatOpenAI::with_config(self.config.clone(), model);
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
