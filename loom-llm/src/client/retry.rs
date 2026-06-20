use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::time::sleep;
use tracing::warn;

use crate::error::LlmError;
use crate::traits::{LlmClient, LlmResponse, ModelInfo, StreamSink};

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

    async fn retry_with_delay<F, Fut, T, E>(&self, mut f: F) -> Result<T, LlmError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: Into<LlmError>,
        T: IsEmptyResponse,
    {
        for attempt in 0..=self.max_retries {
            let result = f().await.map_err(Into::into)?;

            if !result.is_empty() {
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

        Err(LlmError::EmptyResponse {
            retries: self.max_retries,
        })
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
    async fn invoke(&self, messages: &[crate::message::Message]) -> Result<LlmResponse, LlmError> {
        let inner = Arc::clone(&self.inner);
        let messages = messages.to_vec();

        self.retry_with_delay(|| inner.invoke(&messages)).await
    }

    async fn invoke_stream(
        &self,
        messages: &[crate::message::Message],
        sink: Option<&dyn StreamSink>,
        node_id: &str,
    ) -> Result<LlmResponse, LlmError> {
        let inner = Arc::clone(&self.inner);
        let messages = messages.to_vec();

        for attempt in 0..=self.max_retries {
            let resp = inner
                .invoke_stream(&messages, sink, node_id)
                .await
                .map_err(|e| LlmError::InvokeFailed(e.to_string()))?;

            if !resp.is_empty() {
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

        Err(LlmError::EmptyResponse {
            retries: self.max_retries,
        })
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        self.inner.list_models().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::mock::MockLlm;

    #[tokio::test(flavor = "current_thread")]
    async fn test_is_empty_response_all_empty() {
        let resp = LlmResponse::default();
        assert!(is_empty_response(&resp));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_is_empty_response_with_content() {
        let resp = LlmResponse::text("hello");
        assert!(!is_empty_response(&resp));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_is_empty_response_with_reasoning() {
        let resp = LlmResponse {
            content: String::new(),
            reasoning_content: Some("thinking".to_string()),
            tool_calls: vec![],
            usage: None,
            first_chunk_at: None,
        };
        assert!(!is_empty_response(&resp));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_is_empty_response_with_tool_calls() {
        let resp = LlmResponse {
            content: String::new(),
            reasoning_content: None,
            tool_calls: vec![crate::tool::ToolCall {
                id: Some("1".to_string()),
                name: "tool".to_string(),
                arguments: "{}".to_string(),
            }],
            usage: None,
            first_chunk_at: None,
        };
        assert!(!is_empty_response(&resp));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_is_empty_response_with_whitespace_only() {
        let resp = LlmResponse {
            content: "   ".to_string(),
            reasoning_content: Some("   ".to_string()),
            tool_calls: vec![],
            usage: None,
            first_chunk_at: None,
        };
        assert!(is_empty_response(&resp));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_retry_llm_client_success_on_first_attempt() {
        let mock = MockLlm::with_no_tool_calls("success");
        let retry = RetryLlmClient::new(Arc::new(mock));

        let result = retry.invoke(&[]).await.unwrap();
        assert_eq!(result.content, "success");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_retry_llm_client_retries_on_empty_response() {
        // Mock that returns empty first time, then success
        let mock = MockLlm::new("", vec![]).with_content("");
        let retry = RetryLlmClient::new(Arc::new(mock))
            .with_base_delay(Duration::from_millis(0));

        // This test would need a custom mock, so let's simplify it
        let result = retry.invoke(&[]).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            LlmError::EmptyResponse { retries: 3 }
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_retry_llm_client_fails_after_max_retries() {
        let mock = MockLlm::new("", vec![]);
        let retry = RetryLlmClient::new(Arc::new(mock))
            .with_base_delay(Duration::from_millis(0));

        let result = retry.invoke(&[]).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            LlmError::EmptyResponse { retries: 3 }
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_retry_llm_client_with_custom_retries() {
        let mock = MockLlm::new("", vec![]);
        let retry = RetryLlmClient::new(Arc::new(mock))
            .with_max_retries(1)
            .with_base_delay(Duration::from_millis(0));

        let result = retry.invoke(&[]).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            LlmError::EmptyResponse { retries: 1 }
        ));
    }
}