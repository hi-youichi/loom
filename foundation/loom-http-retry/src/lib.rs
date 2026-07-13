//! HTTP retry utilities: exponential backoff and transient error detection.
//!
//! Shared by `loom-llm` and `loom-model-spec` (and any crate that needs to
//! retry transient reqwest failures with capped exponential backoff).

use std::time::Duration;

pub const TRANSIENT_HTTP_MAX_RETRIES: u32 = 10;
pub const TRANSIENT_HTTP_INITIAL_BACKOFF: Duration = Duration::from_millis(500);
pub const TRANSIENT_HTTP_MAX_BACKOFF: Duration = Duration::from_secs(4);

pub fn retry_backoff_for_attempt(attempt: u32) -> Duration {
    let max_secs = TRANSIENT_HTTP_MAX_BACKOFF.as_secs_f64();
    let secs =
        (TRANSIENT_HTTP_INITIAL_BACKOFF.as_secs_f64() * 2_f64.powi(attempt as i32)).min(max_secs);
    Duration::from_secs_f64(secs)
}

pub fn is_retryable_reqwest_error(err: &reqwest::Error) -> bool {
    if err.is_timeout() || err.is_connect() {
        return true;
    }
    looks_like_transient_http_error_message(&err.to_string())
}

pub fn looks_like_transient_http_error_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("incompletemessage")
        || message.contains("connection closed before message completed")
        || message.contains("unexpected eof")
        || message.contains("connection reset")
        || message.contains("broken pipe")
        || message.contains("error decoding")
        || message.contains("error sending request")
        || message.contains("request failed")
        || message.contains("tls")
        || message.contains("ssl")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_incomplete_message_marker() {
        assert!(looks_like_transient_http_error_message(
            "hyper::Error(IncompleteMessage)"
        ));
    }

    #[test]
    fn detects_connection_closed_message() {
        assert!(looks_like_transient_http_error_message(
            "connection closed before message completed"
        ));
    }

    #[test]
    fn detects_decode_error() {
        assert!(looks_like_transient_http_error_message(
            "error decoding response body"
        ));
    }

    #[test]
    fn ignores_non_transient_messages() {
        assert!(!looks_like_transient_http_error_message(
            "dns lookup failed"
        ));
    }

    #[test]
    fn backoff_attempt_zero() {
        assert_eq!(retry_backoff_for_attempt(0), TRANSIENT_HTTP_INITIAL_BACKOFF);
    }

    #[test]
    fn backoff_doubles_each_attempt() {
        let b0 = retry_backoff_for_attempt(0);
        let b1 = retry_backoff_for_attempt(1);
        let b2 = retry_backoff_for_attempt(2);
        assert!(b1 > b0);
        assert!(b2 > b1);
    }

    #[test]
    fn backoff_capped_at_max() {
        let large = retry_backoff_for_attempt(10);
        assert_eq!(large, TRANSIENT_HTTP_MAX_BACKOFF);
    }

    #[test]
    fn detects_unexpected_eof() {
        assert!(looks_like_transient_http_error_message(
            "unexpected eof while reading"
        ));
    }

    #[test]
    fn detects_connection_reset() {
        assert!(looks_like_transient_http_error_message(
            "connection reset by peer"
        ));
    }

    #[test]
    fn detects_broken_pipe() {
        assert!(looks_like_transient_http_error_message("broken pipe"));
    }

    #[test]
    fn detects_error_sending_request() {
        assert!(looks_like_transient_http_error_message(
            "error sending request for url (https://api.example.com/v1/chat/completions)"
        ));
    }

    #[test]
    fn detects_request_failed() {
        assert!(looks_like_transient_http_error_message("request failed"));
    }

    #[test]
    fn detects_tls_error() {
        assert!(looks_like_transient_http_error_message(
            "tls handshake failed"
        ));
    }
}
