//! Default OpenAI-compatible error parser.
//!
//! Covers the ~146 providers in models.dev that follow the OpenAI error protocol:
//! body `{"error":{"message","type","code"}}` with a stable HTTP status mapping.

use model_spec_core::error::{ProviderError, ProviderErrorParser};

use super::parse_openai_compat;

/// Default parser for OpenAI-compatible providers.
pub struct OpenAiCompatParser {
    provider_id: String,
}

impl OpenAiCompatParser {
    /// Creates a parser for the given models.dev provider id.
    pub fn new(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
        }
    }
}

impl ProviderErrorParser for OpenAiCompatParser {
    fn parse(&self, status: u16, _headers: &[(String, String)], body: &[u8]) -> ProviderError {
        parse_openai_compat(&self.provider_id, status, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model_spec_core::error::{ErrorKind, RetryPolicy};

    fn parse(status: u16, body: &[u8]) -> ProviderError {
        OpenAiCompatParser::new("test-provider").parse(status, &[], body)
    }

    #[test]
    fn maps_401_to_auth_failed() {
        let err = parse(
            401,
            br#"{"error":{"message":"bad key","type":"invalid_request_error"}}"#,
        );
        assert_eq!(err.kind, ErrorKind::AuthFailed);
        assert!(matches!(
            err.retry_policy,
            RetryPolicy::NoRetry { action: _ }
        ));
    }

    #[test]
    fn maps_429_rate_limit_to_retryable() {
        let err = parse(
            429,
            br#"{"error":{"message":"rate limit reached","type":"rate_limit_error"}}"#,
        );
        assert_eq!(err.kind, ErrorKind::RateLimited);
        assert!(matches!(err.retry_policy, RetryPolicy::Retry));
    }

    #[test]
    fn maps_429_insufficient_quota_to_quota_exhausted() {
        let err = parse(
            429,
            br#"{"error":{"message":"insufficient_quota","type":"insufficient_quota"}}"#,
        );
        assert_eq!(err.kind, ErrorKind::Billing);
        assert!(matches!(
            err.retry_policy,
            RetryPolicy::NoRetry { action: _ }
        ));
    }

    #[test]
    fn maps_content_filter_type() {
        let err = parse(
            400,
            br#"{"error":{"message":"content filtered","type":"content_filter"}}"#,
        );
        assert_eq!(err.kind, ErrorKind::ContentFilter);
    }

    #[test]
    fn maps_zhipu_business_code_to_quota() {
        // 智谱 429 + 业务码 1310（周限额）→ QuotaExhausted
        let err = parse(
            429,
            br#"{"error":{"message":"weekly quota exhausted","code":"1310"},"id":"x","request_id":"r1"}"#,
        );
        assert_eq!(err.kind, ErrorKind::QuotaExhausted);
        assert_eq!(err.code.as_deref(), Some("1310"));
    }

    #[test]
    fn keeps_raw_message_and_request_id() {
        let err = parse(
            500,
            br#"{"error":{"message":"boom","type":"api_error"},"request_id":"req-1"}"#,
        );
        assert_eq!(err.message, "boom");
        assert_eq!(err.request_id.as_deref(), Some("req-1"));
        assert_eq!(err.kind, ErrorKind::Server);
        assert!(err.is_retryable());
    }
}
