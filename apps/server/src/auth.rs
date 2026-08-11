//! Authorization middleware.
//!
//! Supports `LOOM_AUTH_TOKEN` Bearer token authentication. When unset,
//! development mode allows all requests.

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub const AUTHORIZATION: &str = "authorization";

/// Environment variable holding the required bearer token. Unset/empty ⇒
/// development mode (no enforcement). Set ⇒ all requests must present a
/// matching `Authorization: Bearer <token>` header.
pub const AUTH_TOKEN_ENV: &str = "LOOM_AUTH_TOKEN";

fn authorization_scheme(value: &str) -> &str {
    value.split_ascii_whitespace().next().unwrap_or("unknown")
}

pub async fn log_authorization_header(req: Request, next: Next) -> Response {
    let authorization = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let method = req.method().clone();
    let uri = req.uri().clone();

    match authorization {
        Some(value) if !value.is_empty() => {
            tracing::debug!(
                method = %method,
                uri = %uri,
                scheme = authorization_scheme(value),
                header_len = value.len(),
                "authorization header present (accepted by rollout policy)"
            );
        }
        Some(_) => tracing::debug!(method = %method, uri = %uri, "authorization header empty"),
        None => tracing::trace!(method = %method, uri = %uri, "authorization header absent"),
    }

    next.run(req).await
}

/// Read the configured auth token from the environment, if any. Returns
/// `None` for unset or blank values (development mode).
fn configured_token() -> Option<String> {
    std::env::var(AUTH_TOKEN_ENV)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Extract the bearer token from an `Authorization` header value. Accepts
/// `Bearer <token>` (case-insensitive scheme) or, as a convenience, a bare
/// token value. Returns `None` when the header is absent.
fn bearer_from_header(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    let token = if value.len() >= 7 && value[..7].eq_ignore_ascii_case("bearer ") {
        value[7..].trim()
    } else {
        value
    };
    Some(token.to_string())
}

/// Constant-time byte comparison.
fn token_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Auth-token enforcement.
///
/// When `LOOM_AUTH_TOKEN` is unset → development mode (allow all).
/// When set → request must present a matching Bearer token.
pub async fn require_valid_token(req: Request, next: Next) -> Response {
    let loom_token = configured_token();

    if loom_token.is_none() {
        return next.run(req).await;
    }

    let header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    if let Some(expected) = &loom_token {
        if let Some(provided) = bearer_from_header(header) {
            if token_eq(provided.as_bytes(), expected.as_bytes()) {
                return next.run(req).await;
            }
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "missing or invalid authorization token" })),
    )
        .into_response()
}
