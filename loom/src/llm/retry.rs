use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::warn;

use crate::error::AgentError;
use crate::llm::{LlmClient, LlmResponse, MessageChunk, ModelInfo, ToolCallDelta};

const DEFAULT_MAX_RETRIES: u32 = 3;
const BASE_DELAY: Duration = Duration::from_millis(500);

fn is_empty_response(resp: &LlmResponse) -> bool {
    let content_empty = resp.content.trim().is_empty();
    let reasoning_empty = resp
        .reasoning_content
        .as_ref()
        .is_none_or(|s| s.trim().is_empty());
    let tool_calls_empty = resp.tool_calls.is_empty();
    content_empty && reasoning_empty && tool_calls_empty
}

pub struct RetryLlmClient {
    inner: Arc<dyn LlmClient>,
    max_retries: u32,
    base_delay: Duration,
}

impl RetryLlmClient {
    pub fn new(inner: Arc<dyn LlmClient>) -> Self {
        Self {
            inner,
            max_retries: DEFAULT_MAX_RETRIES,
            base_delay: BASE_DELAY,
        }
    }

    pub fn with_max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    pub fn with_base_delay(mut self, d: Duration) -> Self {
        self.base_delay = d;
        self
    }

    async fn retry_with_delay<F, Fut, T, E>(&self, mut f: F) -> Result<T, AgentError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: Into<AgentError>,
        T: IsEmptyResponse,
    {
        for attempt in 0..=self.max_retries {
            let result = f().await.map_err(Into::into)?;

            if !result.is_empty() || attempt == self.max_retries {
                return Ok(result);
            }

            warn!(
                max_retries = self.max_retries,
                attempt = attempt + 1,
                "empty LLM response, retrying"
            );

            let delay = self.base_delay * 2_u32.pow(attempt);
            sleep(delay).await;
        }

        Err(AgentError::EmptyLlmResponse {
            retries: self.max_retries,
        })
    }

    fn send_chunks_to(
        chunk_tx: &Option<mpsc::Sender<MessageChunk>>,
        resp: &LlmResponse,
    ) {
        if let Some(tx) = chunk_tx {
            if let Some(ref reasoning_content) = resp.reasoning_content {
                if !reasoning_content.is_empty() {
                    let _ = tx
                        .send(MessageChunk::thinking(reasoning_content.clone()))
                        .await;
                }
            }
            if !resp.content.is_empty() {
                let _ = tx
                    .send(MessageChunk::message(resp.content.clone()))
                    .await;
                }
            }
        }
    }
}

trait IsEmptyResponse {
    fn is_empty(&self) -> bool;
}

impl IsEmptyResponse for LlmResponse {
    fn is_empty(&self) -> bool {
        is_empty_response(self)
    }
}

#[async_trait]
impl LlmClient for RetryLlmClient {
    async fn invoke(&self, messages: &[crate::llm::Message]) -> Result<LlmResponse, AgentError> {
        let inner = Arc::clone(&self.inner);
        let messages = messages.to_vec();

        self.retry_with_delay(|| inner.invoke(&messages))
            .await
    }

    async fn invoke_stream(
        &self,
        messages: &[crate::llm::Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
    ) -> Result<LlmResponse, AgentError> {
        let inner = Arc::clone(&self.inner);
        let messages = messages.to_vec();

        for attempt in 0..=self.max_retries {
            let resp = inner
                .invoke_stream(&messages, None)
                .await
                .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;

            if !resp.is_empty() || attempt == self.max_retries {
                Self::send_chunks_to(&chunk_tx, &resp);
                return Ok(resp);
            }

            warn!(
                max_retries = self.max_retries,
                attempt = attempt + 1,
                "empty LLM response in stream mode, retrying"
            );

            let delay = self.base_delay * 2_u32.pow(attempt);
            sleep(delay).await;
        }

        Err(AgentError::EmptyLlmResponse {
            retries: self.max_retries,
        })
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, AgentError> {
        self.inner.list_models().await
    }

    async fn invoke_stream_with_tool_delta(
        &self,
        messages: &[crate::llm::Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
        tool_delta_tx: Option<mpsc::Sender<ToolCallDelta>>,
    ) -> Result<LlmResponse, AgentError> {
        let inner = Arc::clone(&self.inner);
        let messages = messages.to_vec();

        for attempt in 0..=self.max_retries {
            let resp = inner
                .invoke_stream_with_tool_delta(&messages, None, None)
                .await
                .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;

            if !resp.is_empty() || attempt == self.max_retries {
                Self::send_chunks_to(&chunk_tx, &resp);

                if let Some(tx) = &tool_delta_tx {
                    for tool_call in &resp.tool_calls {
                        let _ = tx
                            .send(ToolCallDelta {
                                id: tool_call.id.clone().unwrap_or_default(),
                                name: Some(tool_call.name.clone()),
                                arguments: tool_call.arguments.clone(),
                            })
                            .await;
                    }
                }

                return Ok(resp);
            }

            warn!(
                max_retries = self.max_retries,
                attempt = attempt + 1,
                "empty LLM response in stream with tool delta mode, retrying"
            );

            let delay = self.base_delay * 2_u32.pow(attempt);
            sleep(delay).await;
        }

        Err(AgentError::ExecutionFailed(format!(
            "LLM returned empty response after {} retries in stream with tool delta mode",
            self.max_retries
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::mock::MockLlm;

    #[tokio::test]
    async fn test_is_empty_response_all_empty() {
        let resp = LlmResponse {
            content: String::new(),
            reasoning_content: None,
            tool_calls: vec![],
            usage: None,
        };
        assert!(is_empty_response(&resp));
    }

    #[tokio::test]
    async fn test_is_empty_response_with_content() {
        let resp = LlmResponse {
            content: "hello".to_string(),
            reasoning_content: None,
            tool_calls: vec![],
            usage: None,
        };
        assert!(!is_empty_response(&resp));
    }

    #[tokio::test]
    async fn test_is_empty_response_with_reasoning() {
        let resp = LlmResponse {
            content: String::new(),
            reasoning_content: Some("thinking".to_string()),
            tool_calls: vec![],
            usage: None,
        };
        assert!(!is_empty_response(&resp));
    }

    #[tokio::test]
    async fn test_is_empty_response_with_tool_calls() {
        let resp = LlmResponse {
            content: String::new(),
            reasoning_content: None,
            tool_calls: vec![crate::llm::ToolCall {
                id: Some("1".to_string()),
                name: "tool".to_string(),
                arguments: "{}".to_string(),
            }],
            usage: None,
        };
        assert!(!is_empty_response(&resp));
    }

    #[tokio::test]
    async fn test_is_empty_response_with_whitespace_only() {
        let resp = LlmResponse {
            content: "   ".to_string(),
            reasoning_content: Some("   ".to_string()),
            tool_calls: vec![],
            usage: None,
        };
        assert!(is_empty_response(&resp));
    }

    #[tokio::test]
    async fn test_retry_llm_client_success_on_first_attempt() {
        let mock = MockLlm::with_content("success");
        let retry = RetryLlmClient::new(Arc::new(mock));

        let result = retry.invoke(&[]).await.unwrap();
        assert_eq!(result.content, "success");
    }

    #[tokio::test]
    async fn test_retry_llm_client_retries_on_empty_response() {
        let mock = MockLlm::with_content("").with_second_content("success");
        let retry = RetryLlmClient::new(Arc::new(mock));

        let result = retry.invoke(&[]).await.unwrap();
        assert_eq!(result.content, "success");
    }

    #[tokio::test]
    async fn test_retry_llm_client_fails_after_max_retries() {
        let mock = MockLlm::with_content("");
        let retry = RetryLlmClient::new(Arc::new(mock));

        let result = retry.invoke(&[]).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("empty response after 3 retries"));
    }

    #[tokio::test]
    async fn test_retry_llm_client_with_custom_retries() {
        let mock = MockLlm::with_content("");
        let retry = RetryLlmClient::new(Arc::new(mock))
            .with_max_retries(1);

        let result = retry.invoke(&[]).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("empty response after 1 retries"));
    }
}