use std::sync::Arc;

use async_trait::async_trait;

use crate::error::AgentError;
use crate::traits::{LlmClient, LlmHeaders, LlmProvider};

pub struct CloneableLlmClient(pub Arc<dyn LlmClient>);

#[async_trait]
impl LlmClient for CloneableLlmClient {
    async fn invoke(&self, messages: &[crate::message::Message]) -> Result<crate::traits::LlmResponse, AgentError> {
        self.0.invoke(messages).await
    }

    async fn invoke_stream(
        &self,
        messages: &[crate::message::Message],
        chunk_tx: Option<tokio::sync::mpsc::Sender<crate::traits::MessageChunk>>,
    ) -> Result<crate::traits::LlmResponse, AgentError> {
        self.0.invoke_stream(messages, chunk_tx).await
    }

    async fn invoke_stream_with_tool_delta(
        &self,
        messages: &[crate::message::Message],
        chunk_tx: Option<tokio::sync::mpsc::Sender<crate::traits::MessageChunk>>,
        tool_delta_tx: Option<tokio::sync::mpsc::Sender<crate::traits::ToolCallDelta>>,
    ) -> Result<crate::traits::LlmResponse, AgentError> {
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

    fn create_client_with_headers(
        &self,
        model: &str,
        headers: Option<LlmHeaders>,
    ) -> Result<Box<dyn LlmClient>, AgentError> {
        let _ = headers;
        self.create_client(model)
    }

    fn default_model(&self) -> &str {
        &self.model_id
    }

    fn provider_name(&self) -> &str {
        "fixed"
    }
}
