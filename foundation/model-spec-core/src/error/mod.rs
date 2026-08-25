//! 跨 provider 统一的 LLM 错误模型（类型层）。
//!
//! 本模块只包含纯数据类型与 trait，无 HTTP 类型依赖（`status` 用 `u16`，
//! header 用 `(String, String)`），因此可被 `agent-core` / `anureo-llm`
//! 等多个 crate 共享。运行时解析器实现在 `anureo-llm::error::provider`。

pub mod kind;
pub mod parse;

pub use kind::{ErrorKind, UserAction};
pub use parse::{default_retry_policy, default_user_message, ProviderErrorParser};

use serde::{Deserialize, Serialize};

/// 统一的结构化 provider 错误。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderError {
    /// models.dev provider id，如 "zhipuai"。
    pub provider_id: String,
    /// 规范化错误分类。
    pub kind: ErrorKind,
    /// 原始 HTTP 状态码（SSE 流内错误为 0）。
    pub status: u16,
    /// 原始错误码：error.code / 业务码 / error_type。
    pub code: Option<String>,
    /// 原始错误消息（用于审计/排查）。
    pub message: String,
    /// 人类可读提示（消费端直接展示，可覆盖）。
    pub user_message: String,
    /// 重试决策。
    pub retry_policy: RetryPolicy,
    /// 上游 request id（部分 provider 提供）。
    pub request_id: Option<String>,
    /// 仅 SSE 流内错误：是否已返回部分 token。
    pub partial_tokens: bool,
}

impl ProviderError {
    /// 是否应触发应用层重试。
    pub fn is_retryable(&self) -> bool {
        matches!(
            self.retry_policy,
            RetryPolicy::Retry | RetryPolicy::RetryAfter(_)
        )
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.user_message)
    }
}

/// 重试决策。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryPolicy {
    /// 走统一指数退避重试。
    Retry,
    /// 尊重 `Retry-After` 头，等待指定时长后重试。
    ///
    /// 单位为**毫秒**（`u64`），由 [`crate::error::decide`](anureo_llm::error::decide)
    /// 从 `Retry-After` header 解析后写入。
    RetryAfter(u64),
    /// 重试无效，给用户明确动作。
    NoRetry { action: UserAction },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_error_serde_roundtrip() {
        let err = ProviderError {
            provider_id: "zhipuai".to_string(),
            kind: ErrorKind::Billing,
            status: 429,
            code: Some("1113".to_string()),
            message: "account balance insufficient".to_string(),
            user_message: "账户余额不足".to_string(),
            retry_policy: RetryPolicy::NoRetry {
                action: UserAction::TopUp,
            },
            request_id: Some("req_123".to_string()),
            partial_tokens: false,
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ProviderError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn retry_policy_retry_after_roundtrip() {
        let policy = RetryPolicy::RetryAfter(1500);
        let json = serde_json::to_string(&policy).unwrap();
        let back: RetryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, back);
    }

    #[test]
    fn is_retryable() {
        let mut err = ProviderError {
            provider_id: "x".to_string(),
            kind: ErrorKind::RateLimited,
            status: 429,
            code: None,
            message: String::new(),
            user_message: String::new(),
            retry_policy: RetryPolicy::Retry,
            request_id: None,
            partial_tokens: false,
        };
        assert!(err.is_retryable());

        err.retry_policy = RetryPolicy::NoRetry {
            action: UserAction::TopUp,
        };
        assert!(!err.is_retryable());
    }
}
