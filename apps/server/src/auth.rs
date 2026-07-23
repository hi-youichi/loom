//! Authorization middleware (task P0.7 + LS-017 + OC-compat T2).
//!
//! External mode clients may send Basic or Bearer credentials. This module
//! provides two middlewares:
//!
//! - `log_authorization_header` (P0.7): observability only. Records whether a
//!   credential header was present without ever logging its bytes, and never
//!   blocks a request.
//! - `require_valid_token` (LS-017): real enforcement. Supports two parallel
//!   authentication modes:
//!   - `LOOM_AUTH_TOKEN`: Bearer token. `Authorization: Bearer <token>`.
//!   - `OPENCODE_SERVER_PASSWORD`: Basic auth. `Authorization: Basic base64(user:password)`.
//!
//!   When either is configured, requests must match the configured mode. When
//!   both are unset, development mode allows all requests.

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

/// Environment variable holding the OpenChamber server password (Basic auth).
/// When set, requests must carry `Authorization: Basic base64(opencode:<value>)`.
pub const OC_PASSWORD_ENV: &str = "OPENCODE_SERVER_PASSWORD";

/// Expected username in the Basic auth credential (matches OpenChamber
/// auth-state-runtime.js default).
const OC_BASIC_USER: &str = "opencode";

const BASE64_TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let input = input.trim_end_matches('=');
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for ch in input.bytes() {
        let val = BASE64_TABLE.iter().position(|&b| b == ch)? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
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

fn authorization_scheme(value: &str) -> &str {
    value.split_ascii_whitespace().next().unwrap_or("unknown")
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
    // Strip an optional "Bearer " prefix (case-insensitive).
    let token = if value.len() >= 7 && value[..7].eq_ignore_ascii_case("bearer ") {
        value[7..].trim()
    } else {
        value
    };
    Some(token.to_string())
}

/// Constant-time byte comparison. Not cryptographic, but avoids the trivial
/// short-circuit timing oracle of `==` on the secret token.
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

/// Read the configured OpenChamber server password from the environment.
fn configured_oc_password() -> Option<String> {
    std::env::var(OC_PASSWORD_ENV)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Validate an `Authorization: Basic base64(user:pass)` header against the
/// configured `OPENCODE_SERVER_PASSWORD`. Returns `true` if the password
/// matches (constant-time). Username is not strictly checked but defaults
/// to `opencode` per OpenChamber convention.
fn check_basic_auth(header: Option<&str>, expected_password: &str) -> bool {
    let value = match header.and_then(|h| {
        h.strip_prefix("Basic ")
            .or_else(|| h.strip_prefix("basic "))
    }) {
        Some(v) => v.trim(),
        None => return false,
    };
    let decoded = match base64_decode(value) {
        Some(d) => d,
        None => return false,
    };
    let decoded_str = match std::str::from_utf8(&decoded) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let (user, password) = match decoded_str.split_once(':') {
        Some(up) => up,
        None => return false,
    };
    if !token_eq(user.as_bytes(), OC_BASIC_USER.as_bytes()) {
        return false;
    }
    token_eq(password.as_bytes(), expected_password.as_bytes())
}

/// Real auth-token enforcement (task LS-017 + OC-compat T2).
///
/// Supports two parallel authentication modes:
/// - `LOOM_AUTH_TOKEN` → Bearer token.
/// - `OPENCODE_SERVER_PASSWORD` → Basic auth.
///
/// When neither is configured → development mode (allow all).
/// When one or both are configured → request must match at least one.
pub async fn require_valid_token(req: Request, next: Next) -> Response {
    let loom_token = configured_token();
    let oc_password = configured_oc_password();

    if loom_token.is_none() && oc_password.is_none() {
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

    if let Some(expected) = &oc_password {
        if check_basic_auth(header, expected) {
            return next.run(req).await;
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "missing or invalid authorization token" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_the_authorization_scheme() {
        assert_eq!(authorization_scheme("Basic dXNlcjpwYXNz"), "Basic");
        assert_eq!(authorization_scheme("Bearer secret"), "Bearer");
        assert_eq!(authorization_scheme(""), "unknown");
    }

    #[test]
    fn base64_decode_roundtrip() {
        assert_eq!(
            base64_decode("b3BlbmNvZGU6cGFzcw==").unwrap(),
            b"opencode:pass"
        );
        assert_eq!(base64_decode("dXNlcjpwYXNz").unwrap(), b"user:pass");
        assert_eq!(base64_decode("").unwrap(), b"");
    }

    #[test]
    fn base64_decode_invalid_returns_none() {
        assert!(base64_decode("!!!invalid!!!").is_none());
    }

    #[test]
    fn basic_auth_matching_password() {
        assert!(check_basic_auth(
            Some("Basic b3BlbmNvZGU6c2VjcmV0"),
            "secret"
        ));
    }

    #[test]
    fn basic_auth_wrong_password() {
        assert!(!check_basic_auth(
            Some("Basic b3BlbmNvZGU6d3Jvbmc="),
            "secret"
        ));
    }

    #[test]
    fn basic_auth_no_header() {
        assert!(!check_basic_auth(None, "secret"));
    }

    #[test]
    fn basic_auth_not_basic_scheme() {
        assert!(!check_basic_auth(Some("Bearer token"), "secret"));
    }

    #[test]
    fn token_eq_constant_time() {
        assert!(token_eq(b"abc", b"abc"));
        assert!(!token_eq(b"abc", b"abd"));
        assert!(!token_eq(b"abc", b"ab"));
    }
}
