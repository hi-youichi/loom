//! Retry wrapper for LLM clients.

use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::warn;

use crate::types::message::Message;
use crate::types::error::{LlmError, RetryDecision};
use crate::traits::{LlmClient, LlmResponse, ToolCallDelta, MessageChunk};

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Initial backoff duration.
    pub initial_backoff: Duration,
    /// Maximum backoff duration.
    pub max_backoff: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(1000),
            max_backoff: Duration::from_secs(16),
        }
    }
}

/// Retry wrapper for any LLM client.
pub struct RetryLlmClient<C> {
    /// Inner client.
    inner: C,
    /// Retry configuration.
    config: RetryConfig,
}

impl<C> RetryLlmClient<C> {
    /// Creates a new retry wrapper.
    pub fn new(inner: C) -> Self {
        Self {
            inner,
            config: RetryConfig::default(),
        }
    }

    /// Sets the retry configuration.
    pub fn with_config(mut self, config: RetryConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets the maximum number of retries.
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.config.max_retries = max_retries;
        self
    }

    /// Calculates backoff duration for a given attempt.
    fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let backoff = self.config.initial_backoff.as_millis() as u64 * 2u64.pow(attempt as u32);
        Duration::from_millis(backoff.min(self.config.max_backoff.as_millis() as u64))
    }

    /// Determines if an error is retryable.
    fn is_retryable(&self, error: &LlmError) -> bool {
        error.is_retryable()
    }
}

#[async_trait]
impl<C> LlmClient for RetryLlmClient<C>
where
    C: LlmClient,
{
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, LlmError> {
        let mut attempt = 0u32;
        
        loop {
            match self.inner.invoke(messages).await {
                Ok(response) => return Ok(response),
                Err(error) => {
                    if !self.is_retryable(&error) || attempt >= self.config.max_retries {
                        return Err(error);
                    }
                    
                    let backoff = self.backoff_for_attempt(attempt);
                    warn!(
                        attempt = attempt + 1,
                        max_retries = self.config.max_retries,
                        backoff_ms = backoff.as_millis(),
                        error = %error,
                        "LLM invoke failed, retrying"
                    );
                    
                    tokio::time::sleep(backoff).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn invoke_stream(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
    ) -> Result<LlmResponse, LlmError> {
        self.invoke_stream_with_tool_delta(messages, chunk_tx, None).await
    }

    async fn invoke_stream_with_tool_delta(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
        tool_delta_tx: Option<mpsc::Sender<ToolCallDelta>>,
    ) -> Result<LlmResponse, LlmError> {
        let mut attempt = 0u32;
        
        loop {
            match self.inner.invoke_stream_with_tool_delta(messages, chunk_tx.clone(), tool_delta_tx.clone()).await {
                Ok(response) => return Ok(response),
                Err(error) => {
                    if !self.is_retryable(&error) || attempt >= self.config.max_retries {
                        return Err(error);
                    }
                    
                    let backoff = self.backoff_for_attempt(attempt);
                    warn!(
                        attempt = attempt + 1,
                        max_retries = self.config.max_retries,
                        backoff_ms = backoff.as_millis(),
                        error = %error,
                        "LLM invoke_stream failed, retrying"
                    );
                    
                    tokio::time::sleep(backoff).await;
                    attempt += 1;
                }
            }
        }
    }
}

/// Extension trait for adding retry to any client.
pub trait WithRetry: Sized {
    /// Wraps this client in a retry layer.
    fn with_retry(self) -> RetryLlmClient<Self>;
}

impl<T: LlmClient> WithRetry for T {
    fn with_retry(self) -> RetryLlmClient<Self> {
        RetryLlmClient::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn retry_backoff_exponential() {
        let retry = RetryLlmClient::new(());
        assert_eq!(retry.backoff_for_attempt(0), Duration::from_millis(1000));
        assert_eq!(retry.backoff_for_attempt(1), Duration::from_millis(2000));
        assert_eq!(retry.backoff_for_attempt(2), Duration::from_millis(4000));
    }
}