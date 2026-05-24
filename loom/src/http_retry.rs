use std::time::Duration;

pub(crate) const TRANSIENT_HTTP_MAX_RETRIES: u32 = 10;
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
        || message.contains("error sending request")
        || message.contains("request failed")
        || message.contains("tls")
        || message.contains("ssl")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryDecision {
    Retryable,
    NonRetryable,
}

#[cfg(test)]
pub(crate) fn classify_openai_http_status(status: u16) -> RetryDecision {
    match status {
        429 | 500 | 502 | 503 | 504 | 524 | 598 | 599 => RetryDecision::Retryable,
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
        || message.contains("status code 524")
        || message.contains("status code 598")
        || message.contains("status code 599")
    {
        return RetryDecision::Retryable;
    }

    RetryDecision::NonRetryable
}

// ----- BigModel (Zhipu AI) provider-specific error handling -----
//
// BigModel API uses business error codes in the response body alongside HTTP status codes.
// Reference: https://docs.bigmodel.cn/cn/api/api-code
//
// Retryable business error codes (transient / rate-limit / server-side issues):
//   500   - Internal error
//   1200  - API call error (generic, often transient)
//   1210  - API call parameter error (can be transient for remote model issues)
//   1213  - Parameter not received properly
//   1214  - Parameter illegal (can be transient for remote model issues)
//   1230  - API call flow error
//   1231  - Duplicate request (retry after backoff)
//   1234  - Network error
//   1261  - Prompt too long (retryable if context is trimmed)
//   1302  - Concurrent limit exceeded
//   1303  - Rate limit exceeded
//   1304  - Daily call limit reached
//   1305  - Traffic limit triggered
//   1308  - Usage limit reached (resets at next_flush_time)
//   1310  - Weekly/monthly usage limit reached
//   1312  - Model overloaded (suggests trying later or switching model)
//   1313  - Fair use policy throttling
//
// Non-retryable business error codes (client / auth / permanent issues):
//   1000  - Authentication failed
//   1001  - Missing Authentication header
//   1002  - Invalid Authentication Token
//   1003  - Expired Authentication Token
//   1004  - Authentication Token verification failed
//   1110  - Account inactive
//   1111  - Account does not exist
//   1112  - Account locked
//   1113  - Account in arrears
//   1120  - Cannot access account
//   1121  - Account locked for violation
//   1211  - Model does not exist
//   1212  - Model does not support this method
//   1215  - Mutually exclusive parameters
//   1220  - No permission to access API
//   1221  - API has been taken offline
//   1222  - API does not exist
//   1300  - API call blocked by policy
//   1301  - Content safety filter triggered
//   1309  - Subscription plan expired
//   1311  - Current plan does not have model permission

pub(crate) fn is_bigmodel_url(url: &str) -> bool {
    let url = url.to_ascii_lowercase();
    url.contains("bigmodel.cn") || url.contains("bigmodel.com")
}

pub(crate) fn classify_bigmodel_error_code(code: &str) -> RetryDecision {
    match code {
        "500" | "1200" | "1210" | "1213" | "1214" | "1230" | "1231" | "1234" | "1261"
        | "1302" | "1303" | "1304" | "1305" | "1308" | "1310" | "1312" | "1313" => {
            RetryDecision::Retryable
        }
        _ => RetryDecision::NonRetryable,
    }
}

pub(crate) fn classify_bigmodel_api_error(message: &str) -> RetryDecision {
    let msg_lower = message.to_ascii_lowercase();

    if let Some(code) = extract_error_code(&msg_lower) {
        return classify_bigmodel_error_code(&code);
    }

    if msg_lower.contains("参数非法")
        || msg_lower.contains("并发")
        || msg_lower.contains("频率")
        || msg_lower.contains("流量限制")
        || msg_lower.contains("访问量过大")
        || msg_lower.contains("网络错误")
    {
        return RetryDecision::Retryable;
    }

    RetryDecision::NonRetryable
}

fn extract_error_code(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if let Some(start) = lower.find("(code:") {
        let after = &lower[start + 6..].trim_start();
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return Some(digits);
        }
    }
    None
}

pub(crate) fn is_bigmodel_retryable_status(status: u16, error_body: &str) -> bool {
    if matches!(status, 429 | 500 | 502 | 503 | 504 | 524 | 598 | 599) {
        return true;
    }
    if status == 400 || status == 422 {
        return classify_bigmodel_api_error(error_body) == RetryDecision::Retryable;
    }
    false
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
        assert_eq!(
            classify_openai_http_status(400),
            RetryDecision::NonRetryable
        );
        assert_eq!(
            classify_openai_http_status(401),
            RetryDecision::NonRetryable
        );
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
        assert!(looks_like_transient_http_error_message("tls handshake failed"));
    }

    // --- BigModel-specific tests ---

    #[test]
    fn bigmodel_url_detection() {
        assert!(is_bigmodel_url("https://open.bigmodel.cn/api/paas/v4/chat/completions"));
        assert!(is_bigmodel_url("https://OPEN.BIGMODEL.CN/api/paas/v4"));
        assert!(is_bigmodel_url("https://open.bigmodel.com/api/paas/v4"));
        assert!(!is_bigmodel_url("https://api.openai.com/v1/chat/completions"));
        assert!(!is_bigmodel_url("https://api.anthropic.com/v1/messages"));
    }

    #[test]
    fn bigmodel_retryable_error_codes() {
        assert_eq!(classify_bigmodel_error_code("1214"), RetryDecision::Retryable);
        assert_eq!(classify_bigmodel_error_code("1302"), RetryDecision::Retryable);
        assert_eq!(classify_bigmodel_error_code("1303"), RetryDecision::Retryable);
        assert_eq!(classify_bigmodel_error_code("1305"), RetryDecision::Retryable);
        assert_eq!(classify_bigmodel_error_code("1312"), RetryDecision::Retryable);
        assert_eq!(classify_bigmodel_error_code("1234"), RetryDecision::Retryable);
        assert_eq!(classify_bigmodel_error_code("500"), RetryDecision::Retryable);
        assert_eq!(classify_bigmodel_error_code("1200"), RetryDecision::Retryable);
        assert_eq!(classify_bigmodel_error_code("1210"), RetryDecision::Retryable);
    }

    #[test]
    fn bigmodel_non_retryable_error_codes() {
        assert_eq!(classify_bigmodel_error_code("1002"), RetryDecision::NonRetryable);
        assert_eq!(classify_bigmodel_error_code("1211"), RetryDecision::NonRetryable);
        assert_eq!(classify_bigmodel_error_code("1301"), RetryDecision::NonRetryable);
        assert_eq!(classify_bigmodel_error_code("1309"), RetryDecision::NonRetryable);
        assert_eq!(classify_bigmodel_error_code("1113"), RetryDecision::NonRetryable);
    }

    #[test]
    fn bigmodel_classifies_code_1214_as_retryable() {
        assert_eq!(
            classify_bigmodel_api_error("messages 参数非法。请检查文档。 (code: 1214)"),
            RetryDecision::Retryable
        );
    }

    #[test]
    fn bigmodel_classifies_chinese_rate_limit_as_retryable() {
        assert_eq!(
            classify_bigmodel_api_error("您当前使用该 API 的并发数过高"),
            RetryDecision::Retryable
        );
        assert_eq!(
            classify_bigmodel_api_error("该 API 已触发流量限制"),
            RetryDecision::Retryable
        );
    }

    #[test]
    fn bigmodel_classifies_auth_error_as_non_retryable() {
        assert_eq!(
            classify_bigmodel_api_error("Authentication Token非法 (code: 1002)"),
            RetryDecision::NonRetryable
        );
    }

    #[test]
    fn bigmodel_classifies_content_safety_as_non_retryable() {
        assert_eq!(
            classify_bigmodel_api_error("系统检测到输入或生成内容可能包含不安全或敏感内容 (code: 1301)"),
            RetryDecision::NonRetryable
        );
    }

    #[test]
    fn bigmodel_retryable_status_with_body() {
        assert!(is_bigmodel_retryable_status(
            400,
            "messages 参数非法。请检查文档。 (code: 1214)"
        ));
        assert!(is_bigmodel_retryable_status(429, ""));
        assert!(is_bigmodel_retryable_status(503, ""));
        assert!(!is_bigmodel_retryable_status(400, "Authentication Token非法 (code: 1002)"));
        assert!(!is_bigmodel_retryable_status(401, ""));
    }

    #[test]
    fn generic_classifier_does_not_match_bigmodel_codes() {
        assert_eq!(
            classify_openai_error_message("messages 参数非法。请检查文档。 (code: 1214)"),
            RetryDecision::NonRetryable
        );
    }
}
