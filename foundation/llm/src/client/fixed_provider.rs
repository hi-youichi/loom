use std::sync::Arc;

use async_trait::async_trait;

use crate::error::LlmError;
use loom_graph_core::GraphError;
use crate::traits::{LlmClient, LlmHeaders, LlmProvider};

pub struct CloneableLlmClient(pub Arc<dyn LlmClient>);

#[async_trait]
impl LlmClient for CloneableLlmClient {
    async fn invoke(&self, messages: &[crate::message::Message]) -> Result<crate::traits::LlmResponse, LlmError> {
        self.0.invoke(messages).await
    }

async fn invoke_stream(
        &self,
        messages: &[crate::message::Message],
        sink: Option<&dyn crate::traits::StreamSink>,
        node_id: &str,
    ) -> Result<crate::traits::LlmResponse, LlmError> {
        self.0.invoke_stream(messages, sink, node_id).await
    }
}

pub struct FixedLlmProvider {
    pub client: Arc<dyn LlmClient>,
    pub model_id: String,
}

#[async_trait]
impl LlmProvider for FixedLlmProvider {
    fn create_client(&self, _model: &str) -> Result<Box<dyn LlmClient>, GraphError> {
        Ok(Box::new(CloneableLlmClient(self.client.clone())))
    }

    fn create_client_with_headers(
        &self,
        model: &str,
        headers: Option<LlmHeaders>,
    ) -> Result<Box<dyn LlmClient>, GraphError> {
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
