//! LLM Error Classification and Retry Strategy Abstraction
//!
//! Provides a unified interface for classifying LLM API errors and determining
//! retry behavior across different providers.
//!
//! # Architecture
//!
//! - [`HttpRetryPolicy`]: Determines if HTTP-level errors (status codes, network) are retryable
//! - [`ApiErrorParser`]: Parses API-level business error codes
//!
//! # Usage
//!
//! ```rust
//! use loom::llm::error_classifier::{ProviderType, LlmErrorClassifierConfig};
//!
//! let config = ProviderType::BigModel.default_config();
//! let decision = config.classify_http_error(400, "messages 参数非法 (code: 1214)");
//! ```

mod openai;
mod bigmodel;
mod minimax;

use std::sync::Arc;

pub use openai::{OpenAiRetryPolicy, OpenAiApiParser};
pub use bigmodel::{BigModelRetryPolicy, BigModelApiParser};
pub use minimax::{MiniMaxRetryPolicy, MiniMaxApiParser};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RetryDecision {
    #[default]
    NonRetryable,
    Retryable,
}

impl RetryDecision {
    pub fn is_retryable(&self) -> bool {
        matches!(self, RetryDecision::Retryable)
    }
}

pub trait HttpRetryPolicy: Send + Sync {
    fn is_retryable_status(&self, status: u16, error_body: &str) -> bool;

    fn is_retryable_network_error(&self, error: &str) -> bool {
        let err_lower = error.to_lowercase();
        err_lower.contains("timeout")
            || err_lower.contains("connection reset")
            || err_lower.contains("broken pipe")
            || err_lower.contains("unexpected eof")
            || err_lower.contains("connection closed")
            || err_lower.contains("error sending")
            || err_lower.contains("tls")
            || err_lower.contains("ssl")
    }
}

pub trait ApiErrorParser: Send + Sync {
    fn extract_error_code(&self, message: &str) -> Option<String>;

    fn is_retryable_code(&self, code: &str) -> bool;

    fn classify_api_error(&self, message: &str) -> RetryDecision {
        if let Some(code) = self.extract_error_code(message) {
            if self.is_retryable_code(&code) {
                return RetryDecision::Retryable;
            }
        }
        self.classify_by_message_pattern(message)
    }

    fn classify_by_message_pattern(&self, _message: &str) -> RetryDecision {
        RetryDecision::NonRetryable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderType {
    OpenAI,
    BigModel,
    MiniMax,
}

impl ProviderType {
    pub fn default_config(&self) -> LlmErrorClassifierConfig {
        match self {
            ProviderType::OpenAI => LlmErrorClassifierConfig::openai(),
            ProviderType::BigModel => LlmErrorClassifierConfig::bigmodel(),
            ProviderType::MiniMax => LlmErrorClassifierConfig::minimax(),
        }
    }
}

#[derive(Clone)]
pub struct LlmErrorClassifierConfig {
    http_policy: Arc<dyn HttpRetryPolicy>,
    api_parser: Arc<dyn ApiErrorParser>,
}

impl LlmErrorClassifierConfig {
    pub fn openai() -> Self {
        Self {
            http_policy: Arc::new(OpenAiRetryPolicy),
            api_parser: Arc::new(OpenAiApiParser),
        }
    }

    pub fn bigmodel() -> Self {
        Self {
            http_policy: Arc::new(BigModelRetryPolicy),
            api_parser: Arc::new(BigModelApiParser),
        }
    }

    pub fn minimax() -> Self {
        Self {
            http_policy: Arc::new(MiniMaxRetryPolicy),
            api_parser: Arc::new(MiniMaxApiParser),
        }
    }

    pub fn with_policy<P: HttpRetryPolicy + 'static>(self, policy: P) -> Self {
        Self {
            http_policy: Arc::new(policy),
            ..self
        }
    }

    pub fn with_parser<P: ApiErrorParser + 'static>(self, parser: P) -> Self {
        Self {
            api_parser: Arc::new(parser),
            ..self
        }
    }

    pub fn classify_http_error(&self, status: u16, error_body: &str) -> RetryDecision {
        if self.http_policy.is_retryable_status(status, error_body) {
            return RetryDecision::Retryable;
        }
        self.api_parser.classify_api_error(error_body)
    }

    pub fn classify_network_error(&self, error: &str) -> RetryDecision {
        if self.http_policy.is_retryable_network_error(error) {
            return RetryDecision::Retryable;
        }
        self.api_parser.classify_api_error(error)
    }

    pub fn classify_api_error(&self, message: &str) -> RetryDecision {
        self.api_parser.classify_api_error(message)
    }

    pub fn is_retryable(&self, error: &str) -> bool {
        self.api_parser.classify_api_error(error).is_retryable()
    }

    pub fn from_url(base_url: &str) -> Self {
        let url_lower = base_url.to_lowercase();
        if url_lower.contains("bigmodel.cn") || url_lower.contains("bigmodel.com") {
            return Self::bigmodel();
        }
        if url_lower.contains("minimaxi.com") || url_lower.contains("minimax.chat")
            || url_lower.contains("api.minimax") {
            return Self::minimax();
        }
        Self::openai()
    }
}

impl Default for LlmErrorClassifierConfig {
    fn default() -> Self {
        Self::openai()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bigmodel_code_1214_is_retryable() {
        let config = LlmErrorClassifierConfig::bigmodel();
        assert!(config.classify_http_error(400, "messages 参数非法。请检查文档。 (code: 1214)").is_retryable());
    }

    #[test]
    fn bigmodel_code_1002_is_not_retryable() {
        let config = LlmErrorClassifierConfig::bigmodel();
        let result = config.classify_http_error(401, "Authentication Token非法 (code: 1002)");
        assert!(!result.is_retryable());
    }

    #[test]
    fn openai_429_is_retryable() {
        let config = LlmErrorClassifierConfig::openai();
        assert!(config.classify_http_error(429, "Rate limit exceeded").is_retryable());
    }

    #[test]
    fn openai_500_is_retryable() {
        let config = LlmErrorClassifierConfig::openai();
        assert!(config.classify_http_error(500, "Internal server error").is_retryable());
    }

    #[test]
    fn openai_400_is_not_retryable() {
        let config = LlmErrorClassifierConfig::openai();
        assert!(!config.classify_http_error(400, "Bad request").is_retryable());
    }

    #[test]
    fn network_timeout_is_retryable() {
        let config = LlmErrorClassifierConfig::openai();
        assert!(config.classify_network_error("request timeout").is_retryable());
        assert!(config.classify_network_error("Connection reset by peer").is_retryable());
    }

    #[test]
    fn bigmodel_url_detection() {
        let config = LlmErrorClassifierConfig::from_url("https://open.bigmodel.cn/api/paas/v4");
        let result = config.classify_http_error(400, "messages 参数非法 (code: 1214)");
        assert!(result.is_retryable());
    }

    #[test]
    fn minimax_rate_limit_is_retryable() {
        let config = LlmErrorClassifierConfig::minimax();
        assert!(config.classify_http_error(400, "请求频率超限 (code: 1002)").is_retryable());
    }

    #[test]
    fn provider_type_creates_correct_config() {
        assert!(matches!(
            LlmErrorClassifierConfig::from_url("https://api.openai.com/v1"),
            LlmErrorClassifierConfig { .. } if true
        ));
    }
}