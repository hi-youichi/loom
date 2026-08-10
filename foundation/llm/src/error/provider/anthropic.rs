//! Anthropic 协议默认解析器（覆盖 anthropic / thinkingmachines / freemodel /
//! subconscious / kimi-for-coding）。
//!
//! 错误体 `{"type":"error","error":{"type":"...","message":"..."},"request_id":"..."}`，
//! HTTP 529 = overloaded_error。

use model_spec_core::error::{
    default_retry_policy, default_user_message, ErrorKind, ProviderError, ProviderErrorParser,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct AnthropicErrorDetail {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicErrorBody {
    #[serde(default)]
    error: Option<AnthropicErrorDetail>,
    #[serde(default)]
    request_id: Option<String>,
}

/// Anthropic-protocol parser.
pub struct AnthropicParser {
    provider_id: String,
}

impl AnthropicParser {
    pub fn new(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
        }
    }
}

fn kind_from_status(status: u16) -> ErrorKind {
    match status {
        400 => ErrorKind::BadRequest,
        401 => ErrorKind::AuthFailed,
        402 => ErrorKind::Billing,
        403 => ErrorKind::Permission,
        404 => ErrorKind::NotFound,
        409 => ErrorKind::BadRequest,
        413 => ErrorKind::RequestTooLarge,
        429 => ErrorKind::RateLimited,
        500 => ErrorKind::Server,
        504 => ErrorKind::Server,
        529 => ErrorKind::Overloaded,
        _ => ErrorKind::Unknown,
    }
}

fn kind_from_error_type(ty: &str) -> Option<ErrorKind> {
    match ty {
        "invalid_request_error" => Some(ErrorKind::BadRequest),
        "authentication_error" => Some(ErrorKind::AuthFailed),
        "billing_error" => Some(ErrorKind::Billing),
        "permission_error" => Some(ErrorKind::Permission),
        "not_found_error" => Some(ErrorKind::NotFound),
        "request_too_large" => Some(ErrorKind::RequestTooLarge),
        "rate_limit_error" => Some(ErrorKind::RateLimited),
        "api_error" => Some(ErrorKind::Server),
        "timeout_error" => Some(ErrorKind::Server),
        "overloaded_error" => Some(ErrorKind::Overloaded),
        _ => None,
    }
}

impl ProviderErrorParser for AnthropicParser {
    fn parse(&self, status: u16, _headers: &[(String, String)], body: &[u8]) -> ProviderError {
        let mut kind = kind_from_status(status);
        let mut message = String::new();
        let mut request_id = None;

        if let Ok(resp) = serde_json::from_slice::<AnthropicErrorBody>(body) {
            if let Some(detail) = resp.error {
                message = detail.message.unwrap_or_default();
                if let Some(ty) = &detail.r#type {
                    if let Some(k) = kind_from_error_type(ty) {
                        kind = k;
                    }
                }
            }
            request_id = resp.request_id;
        }

        let retry_policy = default_retry_policy(kind);

        ProviderError {
            provider_id: self.provider_id.clone(),
            kind,
            status,
            code: None,
            message,
            user_message: default_user_message(kind),
            retry_policy,
            request_id,
            partial_tokens: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model_spec_core::error::RetryPolicy;

    fn parse(status: u16, body: &[u8]) -> ProviderError {
        AnthropicParser::new("anthropic").parse(status, &[], body)
    }

    #[test]
    fn maps_529_to_overloaded() {
        let err = parse(
            529,
            br#"{"type":"error","error":{"type":"overloaded_error","message":"overloaded"}}"#,
        );
        assert_eq!(err.kind, ErrorKind::Overloaded);
        assert!(matches!(err.retry_policy, RetryPolicy::Retry));
    }

    #[test]
    fn maps_402_to_billing() {
        let err = parse(
            402,
            br#"{"type":"error","error":{"type":"billing_error","message":"bill"}}"#,
        );
        assert_eq!(err.kind, ErrorKind::Billing);
        assert!(!err.is_retryable());
    }

    #[test]
    fn maps_404_to_not_found() {
        let err = parse(404, br#"{"type":"error","error":{"type":"not_found_error","message":"nope"},"request_id":"req-1"}"#);
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert_eq!(err.request_id.as_deref(), Some("req-1"));
    }
}
