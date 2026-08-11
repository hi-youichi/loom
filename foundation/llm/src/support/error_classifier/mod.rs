//! LLM 网络错误重试判定（收敛后）
//!
//! 收敛说明（2025-08）：业务码 / HTTP 错误体分类已迁移至 [`crate::error::provider`]
//! （`parser_for` + 各覆写解析器 + `decide`）。本模块**仅保留传输层网络错误判定**，
//! 供 async_openai 路径（`crate::client::openai`）在收到非结构化错误时决定是否重试。

mod openai;

use std::sync::Arc;

pub use openai::OpenAiRetryPolicy;

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

/// 传输层网络错误重试判定。
pub trait HttpRetryPolicy: Send + Sync {
    /// 网络错误消息（连接/超时/TLS 等）是否可重试。
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

#[derive(Clone)]
pub struct LlmErrorClassifierConfig {
    http_policy: Arc<dyn HttpRetryPolicy>,
}

impl LlmErrorClassifierConfig {
    pub fn openai() -> Self {
        Self {
            http_policy: Arc::new(OpenAiRetryPolicy),
        }
    }

    /// 传输层网络错误是否可重试（连接/超时/TLS）。
    pub fn classify_network_error(&self, error: &str) -> RetryDecision {
        if self.http_policy.is_retryable_network_error(error) {
            RetryDecision::Retryable
        } else {
            RetryDecision::NonRetryable
        }
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
    fn network_timeout_is_retryable() {
        let config = LlmErrorClassifierConfig::openai();
        assert!(config
            .classify_network_error("request timeout")
            .is_retryable());
        assert!(config
            .classify_network_error("Connection reset by peer")
            .is_retryable());
        assert!(config.classify_network_error("broken pipe").is_retryable());
    }

    #[test]
    fn non_network_errors_are_not_retryable() {
        let config = LlmErrorClassifierConfig::openai();
        assert!(!config
            .classify_network_error("invalid api key")
            .is_retryable());
    }
}
