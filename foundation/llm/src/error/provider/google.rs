//! Google / Vertex（Gemini）错误解析器。
//!
//! REST 错误体 `{"error":{"code":<http>,"message":"...","status":"INVALID_ARGUMENT","details":[...]}}`。
//! `status`（gRPC 枚举）优先于 HTTP 码。

use model_spec_core::error::{
    default_retry_policy, default_user_message, ErrorKind, ProviderError, ProviderErrorParser,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct GoogleError {
    #[serde(default)]
    #[allow(dead_code)]
    code: Option<i64>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct GoogleErrorBody {
    #[serde(default)]
    error: Option<GoogleError>,
}

/// Google Gemini / Vertex REST parser.
pub struct GoogleParser {
    provider_id: String,
}

impl GoogleParser {
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
        403 => ErrorKind::Permission,
        404 => ErrorKind::NotFound,
        429 => ErrorKind::RateLimited,
        500 => ErrorKind::Server,
        503 => ErrorKind::Overloaded,
        504 => ErrorKind::Server,
        _ => ErrorKind::Unknown,
    }
}

fn kind_from_grpc_status(status: &str) -> Option<ErrorKind> {
    Some(match status {
        "INVALID_ARGUMENT" | "FAILED_PRECONDITION" | "OUT_OF_RANGE" => ErrorKind::BadRequest,
        "UNAUTHENTICATED" => ErrorKind::AuthFailed,
        "PERMISSION_DENIED" => ErrorKind::Permission,
        "NOT_FOUND" => ErrorKind::NotFound,
        "ABORTED" | "ALREADY_EXISTS" | "CONFLICT" => ErrorKind::BadRequest,
        "RESOURCE_EXHAUSTED" => ErrorKind::RateLimited,
        "INTERNAL" | "DATA_LOSS" | "UNKNOWN" => ErrorKind::Server,
        "UNAVAILABLE" => ErrorKind::Overloaded,
        "DEADLINE_EXCEEDED" => ErrorKind::Server,
        _ => return None,
    })
}

impl ProviderErrorParser for GoogleParser {
    fn parse(&self, status: u16, _headers: &[(String, String)], body: &[u8]) -> ProviderError {
        let mut kind = kind_from_status(status);
        let mut message = String::new();

        if let Ok(resp) = serde_json::from_slice::<GoogleErrorBody>(body) {
            if let Some(detail) = resp.error {
                if let Some(s) = &detail.status {
                    if let Some(k) = kind_from_grpc_status(s) {
                        kind = k;
                    }
                }
                message = detail.message.unwrap_or_default();
            }
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
            request_id: None,
            partial_tokens: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(status: u16, body: &[u8]) -> ProviderError {
        GoogleParser::new("google").parse(status, &[], body)
    }

    #[test]
    fn maps_grpc_status_resource_exhausted() {
        let err = parse(429, br#"{"error":{"code":429,"message":"quota","status":"RESOURCE_EXHAUSTED"}}"#);
        assert_eq!(err.kind, ErrorKind::RateLimited);
        assert!(err.is_retryable());
    }

    #[test]
    fn maps_grpc_status_permission_denied() {
        let err = parse(403, br#"{"error":{"code":403,"message":"denied","status":"PERMISSION_DENIED"}}"#);
        assert_eq!(err.kind, ErrorKind::Permission);
    }

    #[test]
    fn maps_401_by_status() {
        let err = parse(401, br#"{"error":{"code":401,"message":"bad key"}}"#);
        assert_eq!(err.kind, ErrorKind::AuthFailed);
    }
}
