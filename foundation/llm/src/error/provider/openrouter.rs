//! OpenRouter 覆写（openrouter.ai）。
//!
//! 错误体 `{"error":{"code":<int>,"message":"...","metadata":{"error_type":"...","provider_code":"..."}}}`。
//! `metadata.error_type` 是归一化词汇，**优先于** HTTP 状态码判定。

use model_spec_core::error::{ErrorKind, ProviderError, ProviderErrorParser};
use serde::Deserialize;

use super::parse_openai_compat;

#[derive(Deserialize)]
struct OpenRouterError {
    #[serde(default)]
    #[allow(dead_code)]
    code: Option<i32>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    metadata: Option<OpenRouterMetadata>,
}

#[derive(Deserialize)]
struct OpenRouterMetadata {
    #[serde(default)]
    error_type: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    provider_code: Option<String>,
}

#[derive(Deserialize)]
struct OpenRouterBody {
    #[serde(default)]
    error: Option<OpenRouterError>,
}

/// OpenRouter override: normalized `error_type` takes priority over HTTP status.
pub struct OpenRouterParser {
    provider_id: String,
}

impl OpenRouterParser {
    pub fn new(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
        }
    }
}

fn kind_from_error_type(ty: &str) -> Option<ErrorKind> {
    match ty {
        "insufficient_quota" => Some(ErrorKind::Billing),
        "rate_limit_exceeded" => Some(ErrorKind::RateLimited),
        "context_length_exceeded" | "invalid_request" | "invalid_prompt" | "prompt_too_long" => {
            Some(ErrorKind::BadRequest)
        }
        "authentication" => Some(ErrorKind::AuthFailed),
        "permission" => Some(ErrorKind::Permission),
        "not_found" => Some(ErrorKind::NotFound),
        "server_error" | "internal_error" | "bad_gateway" | "provider_switching" => {
            Some(ErrorKind::Server)
        }
        "overloaded" | "provider_failed" => Some(ErrorKind::Overloaded),
        "timeout" => Some(ErrorKind::Server),
        _ => None,
    }
}

impl ProviderErrorParser for OpenRouterParser {
    fn parse(&self, status: u16, _headers: &[(String, String)], body: &[u8]) -> ProviderError {
        let mut err = parse_openai_compat(&self.provider_id, status, body);
        if let Ok(resp) = serde_json::from_slice::<OpenRouterBody>(body) {
            if let Some(e) = resp.error {
                if let Some(ty) = e.metadata.and_then(|m| m.error_type) {
                    if let Some(k) = kind_from_error_type(&ty) {
                        err.kind = k;
                        err.retry_policy = model_spec_core::error::default_retry_policy(k);
                        err.user_message =
                            model_spec_core::error::default_user_message(k);
                        err.code = Some(ty);
                    }
                }
                if let Some(msg) = e.message {
                    if !msg.is_empty() {
                        err.message = msg;
                    }
                }
            }
        }
        err
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(status: u16, body: &[u8]) -> ProviderError {
        OpenRouterParser::new("openrouter").parse(status, &[], body)
    }

    #[test]
    fn error_type_wins_over_status() {
        // 429 但 error_type=insufficient_quota → Billing
        let err = parse(
            429,
            br#"{"error":{"code":429,"message":"quota","metadata":{"error_type":"insufficient_quota","provider_code":"quota_exceeded"}}}"#,
        );
        assert_eq!(err.kind, ErrorKind::Billing);
        assert!(!err.is_retryable());
    }

    #[test]
    fn overloaded_error_type() {
        let err = parse(
            503,
            br#"{"error":{"code":503,"message":"no providers","metadata":{"error_type":"provider_failed"}}}"#,
        );
        assert_eq!(err.kind, ErrorKind::Overloaded);
        assert!(err.is_retryable());
    }

    #[test]
    fn falls_back_to_status_when_no_type() {
        let err = parse(401, br#"{"error":{"code":401,"message":"bad key"}}"#);
        assert_eq!(err.kind, ErrorKind::AuthFailed);
    }
}
