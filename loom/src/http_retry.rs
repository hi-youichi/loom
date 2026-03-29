use std::time::Duration;

pub(crate) const TRANSIENT_HTTP_MAX_RETRIES: u32 = 5;
pub(crate) const TRANSIENT_HTTP_INITIAL_BACKOFF: Duration = Duration::from_millis(500);
pub(crate) const TRANSIENT_HTTP_MAX_BACKOFF: Duration = Duration::from_secs(4);

pub(crate) fn retry_backoff_for_attempt(attempt: u32) -> Duration {
    let secs = TRANSIENT_HTTP_INITIAL_BACKOFF.as_secs_f64() * 2_f64.powi(attempt as i32);
    Duration::from_secs_f64(secs).min(TRANSIENT_HTTP_MAX_BACKOFF)
}

pub(crate) fn is_retryable_reqwest_error(err: &reqwest::Error) -> bool {
    if err.is_timeout() || err.is_connect() {
        return true;
    }
    looks_like_transient_http_error_message(&err.to_string())
}

pub(crate) fn looks_like_transient_http_error_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("incompletemessage")
        || message.contains("connection closed before message completed")
        || message.contains("unexpected eof")
        || message.contains("connection reset")
        || message.contains("broken pipe")
        || message.contains("error decoding")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryDecision {
    Retryable,
    NonRetryable,
}

#[cfg(test)]
pub(crate) fn classify_openai_http_status(status: u16) -> RetryDecision {
    match status {
        429 | 500 | 502 | 503 | 504 => RetryDecision::Retryable,
        _ => RetryDecision::NonRetryable,
    }
}

pub(crate) fn classify_openai_error_message(message: &str) -> RetryDecision {
    if looks_like_transient_http_error_message(message) {
        return RetryDecision::Retryable;
    }

    let message = message.to_ascii_lowercase();
    if message.contains("status code 429")
        || message.contains("status code 500")
        || message.contains("status code 502")
        || message.contains("status code 503")
        || message.contains("status code 504")
    {
        return RetryDecision::Retryable;
    }

    RetryDecision::NonRetryable
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
    fn classifies_retryable_openai_statuses() {
        assert_eq!(classify_openai_http_status(429), RetryDecision::Retryable);
        assert_eq!(classify_openai_http_status(500), RetryDecision::Retryable);
        assert_eq!(classify_openai_http_status(503), RetryDecision::Retryable);
    }

    #[test]
    fn classifies_non_retryable_openai_statuses() {
        assert_eq!(classify_openai_http_status(400), RetryDecision::NonRetryable);
        assert_eq!(classify_openai_http_status(401), RetryDecision::NonRetryable);
    }

    #[test]
    fn classifies_retryable_openai_error_messages() {
        assert_eq!(
            classify_openai_error_message("HTTP status code 429 Too Many Requests"),
            RetryDecision::Retryable
        );
        assert_eq!(
            classify_openai_error_message("HTTP status code 503 Service Unavailable"),
            RetryDecision::Retryable
        );
    }

    #[test]
    fn classifies_non_retryable_openai_error_messages() {
        assert_eq!(
            classify_openai_error_message(
                "HTTP status client error (400 Bad Request): messages with role 'tool' must be a response to a preceding message with 'tool_calls'"
            ),
            RetryDecision::NonRetryable
        );
    }
}
