use std::sync::Arc;

use async_trait::async_trait;

use crate::error::AgentError;
use crate::llm::{LlmClient, LlmProvider};
use crate::model_spec::ModelTier;

pub struct CloneableLlmClient(pub Arc<dyn LlmClient>);

#[async_trait]
impl LlmClient for CloneableLlmClient {
    async fn invoke(&self, messages: &[crate::message::Message]) -> Result<crate::llm::LlmResponse, AgentError> {
        self.0.invoke(messages).await
    }

    async fn invoke_stream(
        &self,
        messages: &[crate::message::Message],
        chunk_tx: Option<tokio::sync::mpsc::Sender<crate::stream::MessageChunk>>,
    ) -> Result<crate::llm::LlmResponse, AgentError> {
        self.0.invoke_stream(messages, chunk_tx).await
    }

    async fn invoke_stream_with_tool_delta(
        &self,
        messages: &[crate::message::Message],
        chunk_tx: Option<tokio::sync::mpsc::Sender<crate::stream::MessageChunk>>,
        tool_delta_tx: Option<tokio::sync::mpsc::Sender<crate::llm::ToolCallDelta>>,
    ) -> Result<crate::llm::LlmResponse, AgentError> {
        self.0.invoke_stream_with_tool_delta(messages, chunk_tx, tool_delta_tx).await
    }
}

pub struct FixedLlmProvider {
    pub client: Arc<dyn LlmClient>,
    pub model_id: String,
}

#[async_trait]
impl LlmProvider for FixedLlmProvider {
    fn create_client(&self, _model: &str) -> Result<Box<dyn LlmClient>, AgentError> {
        Ok(Box::new(CloneableLlmClient(self.client.clone())))
    }

    fn default_model(&self) -> &str {
        &self.model_id
    }

    fn provider_name(&self) -> &str {
        "fixed"
    }

    async fn resolve_tier(&self, tier: ModelTier) -> Result<String, AgentError> {
        if tier == ModelTier::None {
            return Ok(self.default_model().to_string());
        }
        Ok(self.default_model().to_string())
    }
}
