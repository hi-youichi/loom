//! Retry configuration and API error-body parsing for OpenAI-compatible clients.
//!
//! Centralises the retry constants, backoff calculator, status-code classifier
//! helper, and structured JSON error-body formatter that both [`super::ChatOpenAICompat`]
//! request and streaming paths use.

use crate::support::error_classifier::LlmErrorClassifierConfig;

/// Example default base URL (Zhipu BigModel OpenAI-compatible API).
pub(crate) const DEFAULT_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";

/// Max retries for retryable 5xx (500, 502, 503, 504). Total attempts = 1 + this.
pub(crate) const COMPAT_RETRY_MAX_RETRIES: u32 = 20;
/// Initial backoff before first retry.
pub(crate) const COMPAT_RETRY_INITIAL_BACKOFF: std::time::Duration =
    std::time::Duration::from_secs(1);
/// Max backoff cap.
pub(crate) const COMPAT_RETRY_MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(16);

/// Returns `true` if the HTTP status code is retryable for the given provider.
pub(crate) fn is_retryable_status_for(
    status: reqwest::StatusCode,
    base_url: &str,
    error_body: &str,
) -> bool {
    let classifier = LlmErrorClassifierConfig::from_url(base_url);
    classifier
        .classify_http_error(status.as_u16(), error_body)
        .is_retryable()
}

/// Exponential backoff duration for the given 0-based attempt index.
pub(crate) fn backoff_for_attempt(attempt: u32) -> std::time::Duration {
    let max_secs = COMPAT_RETRY_MAX_BACKOFF.as_secs_f64();
    let secs =
        (COMPAT_RETRY_INITIAL_BACKOFF.as_secs_f64() * 2_f64.powi(attempt as i32)).min(max_secs);
    std::time::Duration::from_secs_f64(secs)
}

// ---------------------------------------------------------------------------
// API error response parsing
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct ApiErrorDetail {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

#[derive(serde::Deserialize)]
struct ApiErrorResponse {
    error: Option<ApiErrorDetail>,
}

/// Format an API error body into a concise, human-readable string.
///
/// Tries to parse the body as `{"error":{"message":"...","code":"...","type":"..."}}`.
/// Falls back to the raw body if parsing fails or the error object has no useful fields.
pub(crate) fn format_api_error_body(body: &[u8]) -> String {
    if let Ok(resp) = serde_json::from_slice::<ApiErrorResponse>(body) {
        if let Some(detail) = resp.error {
            let msg = detail.message.unwrap_or_default();
            if !msg.is_empty() {
                if let Some(code) = &detail.code {
                    return format!("{} (code: {})", msg, code);
                }
                if let Some(ty) = &detail.r#type {
                    return format!("{} (type: {})", msg, ty);
                }
                return msg;
            }
            if let Some(code) = &detail.code {
                return format!("error code: {}", code);
            }
            if let Some(ty) = &detail.r#type {
                return format!("error type: {}", ty);
            }
        }
    }
    String::from_utf8_lossy(body).into_owned()
}

#[cfg(test)]
mod tests {
    use super::format_api_error_body;

    #[test]
    fn format_api_error_body_parses_aliyun_arrearage_error() {
        let body = br#"{"error":{"message":"Access denied, please make sure your account is in good standing. For details, see: https://help.aliyun.com/zh/model-studio/error-code#overdue-payment","type":"Arrearage","param":null,"code":"Arrearage"},"id":"chatcmpl-test","request_id":"test"}"#;
        let result = format_api_error_body(body);
        assert!(
            result.contains("Access denied"),
            "should contain message: {}",
            result
        );
        assert!(
            result.contains("(code: Arrearage)"),
            "should contain code: {}",
            result
        );
        assert!(
            !result.contains("\"error\""),
            "should not contain raw JSON: {}",
            result
        );
    }

    #[test]
    fn format_api_error_body_parses_openai_style_error() {
        let body = br#"{"error":{"message":"Rate limit exceeded","type":"rate_limit_error","param":null,"code":"rate_limit_exceeded"}}"#;
        let result = format_api_error_body(body);
        assert_eq!(result, "Rate limit exceeded (code: rate_limit_exceeded)");
    }

    #[test]
    fn format_api_error_body_parses_error_with_type_only() {
        let body = br#"{"error":{"message":"Something went wrong","type":"server_error"}}"#;
        let result = format_api_error_body(body);
        assert_eq!(result, "Something went wrong (type: server_error)");
    }

    #[test]
    fn format_api_error_body_falls_back_to_raw_on_invalid_json() {
        let body = b"not json at all";
        let result = format_api_error_body(body);
        assert_eq!(result, "not json at all");
    }

    #[test]
    fn format_api_error_body_handles_empty_error_object() {
        let body = br#"{"error":{}}"#;
        let result = format_api_error_body(body);
        assert_eq!(result, "{\"error\":{}}");
    }

    #[test]
    fn format_api_error_body_handles_missing_error_field() {
        let body = br#"{"message":"something"}"#;
        let result = format_api_error_body(body);
        assert_eq!(result, "{\"message\":\"something\"}");
    }
}
