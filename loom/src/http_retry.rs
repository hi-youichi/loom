use std::time::Duration;

pub(crate) const TRANSIENT_HTTP_MAX_RETRIES: u32 = 3;
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
    fn ignores_non_transient_messages() {
        assert!(!looks_like_transient_http_error_message(
            "dns lookup failed"
        ));
    }
}
