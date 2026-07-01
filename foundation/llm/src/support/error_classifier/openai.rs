//! OpenAI Error Classification Strategy
//!
//! Standard OpenAI API uses HTTP status codes for retry decisions.
//! No business error codes are used.

use super::{ApiErrorParser, HttpRetryPolicy};

pub struct OpenAiRetryPolicy;

impl HttpRetryPolicy for OpenAiRetryPolicy {
    fn is_retryable_status(&self, status: u16, _error_body: &str) -> bool {
        matches!(status, 429 | 500..=504 | 524 | 598 | 599)
    }
}

pub struct OpenAiApiParser;

impl ApiErrorParser for OpenAiApiParser {
    fn extract_error_code(&self, _message: &str) -> Option<String> {
        None
    }

    fn is_retryable_code(&self, _code: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_status_codes() {
        let policy = OpenAiRetryPolicy;
        assert!(policy.is_retryable_status(429, ""));
        assert!(policy.is_retryable_status(500, ""));
        assert!(policy.is_retryable_status(502, ""));
        assert!(policy.is_retryable_status(503, ""));
        assert!(policy.is_retryable_status(504, ""));
        assert!(policy.is_retryable_status(524, ""));
    }

    #[test]
    fn non_retryable_status_codes() {
        let policy = OpenAiRetryPolicy;
        assert!(!policy.is_retryable_status(400, "Bad request"));
        assert!(!policy.is_retryable_status(401, "Unauthorized"));
        assert!(!policy.is_retryable_status(403, "Forbidden"));
        assert!(!policy.is_retryable_status(404, "Not found"));
    }

    #[test]
    fn network_errors_are_retryable() {
        let policy = OpenAiRetryPolicy;
        assert!(policy.is_retryable_network_error("request timeout"));
        assert!(policy.is_retryable_network_error("connection reset by peer"));
        assert!(policy.is_retryable_network_error("broken pipe"));
        assert!(policy.is_retryable_network_error("unexpected eof while reading"));
        assert!(policy.is_retryable_network_error("tls handshake failed"));
    }
}