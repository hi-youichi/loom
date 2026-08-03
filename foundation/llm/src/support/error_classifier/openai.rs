//! OpenAI 传输层网络错误重试判定。
//!
//! 收敛说明：业务码 / HTTP 状态分类已迁移至 [`crate::error::provider`]。
//! 本模块仅实现网络错误判定（连接/超时/TLS），供 async_openai 路径使用。

use super::HttpRetryPolicy;

pub struct OpenAiRetryPolicy;

impl HttpRetryPolicy for OpenAiRetryPolicy {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_errors_are_retryable() {
        let policy = OpenAiRetryPolicy;
        assert!(policy.is_retryable_network_error("request timeout"));
        assert!(policy.is_retryable_network_error("connection reset by peer"));
        assert!(policy.is_retryable_network_error("broken pipe"));
        assert!(policy.is_retryable_network_error("unexpected eof while reading"));
        assert!(policy.is_retryable_network_error("tls handshake failed"));
    }

    #[test]
    fn non_network_errors_are_not_retryable() {
        let policy = OpenAiRetryPolicy;
        assert!(!policy.is_retryable_network_error("invalid api key"));
        assert!(!policy.is_retryable_network_error("rate limit reached"));
    }
}
