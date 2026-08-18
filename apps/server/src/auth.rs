//! Authorization middleware.
//!
//! Two credential modes, combinable:
//!
//! 1. `LOOM_AUTH_TOKEN` — programmatic `Authorization: Bearer <token>`.
//! 2. `LOOMDESK_JWT_SECRET` — verify the `oc_ui_session` cookie minted by the
//!    Loom Desk Express ui-auth controller (HS256 JWT, `type: ui-session`).
//!    Login, logout, rate limiting, and passkeys live in Express; loom.exe
//!    only verifies signatures. Note: if Express rotates its JWT secret
//!    (global sign-out), loom.exe must be restarted to pick up the new value.
//!
//! When neither variable is set, development mode allows all requests.

use axum::{
    extract::Request,
    http::{
        header::COOKIE,
        StatusCode,
    },
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub const AUTHORIZATION: &str = "authorization";

/// Environment variable holding the required bearer token. Unset/empty ⇒
/// not enforced. Set ⇒ requests must present a matching
/// `Authorization: Bearer <token>` header.
pub const AUTH_TOKEN_ENV: &str = "LOOM_AUTH_TOKEN";

/// Environment variable holding the Loom Desk Express JWT secret (contents of
/// `<data-dir>/jwt-secret`). Unset/empty ⇒ cookie auth not enforced.
pub const JWT_SECRET_ENV: &str = "LOOMDESK_JWT_SECRET";

/// Session cookie name minted by Express ui-auth (`SESSION_COOKIE_NAME`).
pub const UI_SESSION_COOKIE: &str = "oc_ui_session";

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

/// Read the configured Express JWT secret from the environment, if any.
fn configured_jwt_secret() -> Option<String> {
    std::env::var(JWT_SECRET_ENV)
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

/// Extract the UI session cookie value from a request's Cookie header.
///
/// Express sets the value with `encodeURIComponent`; JWT base64url characters
/// (`A-Za-z0-9-_` and `.`) are never percent-encoded by it, so the raw value
/// equals the encoded value and no percent-decoding is needed.
fn ui_session_cookie_value<B>(req: &axum::http::Request<B>) -> Option<&str> {
    req.headers()
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())?
        .split(';')
        .filter_map(|part| part.trim().strip_prefix(UI_SESSION_COOKIE).and_then(|rest| rest.strip_prefix('=')))
        .next()
}

/// Verify an Express-issued UI session JWT: HS256, unexpired, and carrying
/// the `type: ui-session` claim.
fn is_valid_ui_session(cookie_value: &str, secret: &str) -> bool {
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.validate_exp = true;
    // Express signs with jose, which applies no leeway by default. Match it.
    validation.leeway = 0;
    validation.required_spec_claims = ["exp".to_string()].into_iter().collect();
    match jsonwebtoken::decode::<serde_json::Value>(
        cookie_value,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    ) {
        Ok(data) => data
            .claims
            .get("type")
            .and_then(|claim| claim.as_str())
            == Some("ui-session"),
        Err(_) => false,
    }
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

/// Auth enforcement.
///
/// When neither `LOOM_AUTH_TOKEN` nor `LOOMDESK_JWT_SECRET` is set →
/// development mode (allow all). Otherwise a request must present either a
/// matching Bearer token or a valid `oc_ui_session` session cookie.
pub async fn require_valid_token(req: Request, next: Next) -> Response {
    let loom_token = configured_token();
    let jwt_secret = configured_jwt_secret();

    if loom_token.is_none() && jwt_secret.is_none() {
        return next.run(req).await;
    }

    if let Some(expected) = &loom_token {
        let header = req
            .headers()
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        if let Some(provided) = bearer_from_header(header) {
            if token_eq(provided.as_bytes(), expected.as_bytes()) {
                return next.run(req).await;
            }
        }
    }

    if let Some(secret) = &jwt_secret {
        if let Some(cookie_value) = ui_session_cookie_value(&req) {
            if is_valid_ui_session(cookie_value, secret) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request as HttpRequest;

    const SECRET: &str = "test-secret";

    fn signed_session(claims: &serde_json::Value) -> String {
        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
        jsonwebtoken::encode(
            &header,
            claims,
            &jsonwebtoken::EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .expect("sign jwt")
    }

    fn ui_session_claims() -> serde_json::Value {
        json!({
            "type": "ui-session",
            "iat": 1700000000_u64,
            "exp": (chrono::Utc::now().timestamp() + 3600) as u64,
        })
    }

    #[test]
    fn ui_session_cookie_is_extracted_from_cookie_header() {
        let req = HttpRequest::builder()
            .header(COOKIE, "other=1; oc_ui_session=abc.def.ghi; more=2")
            .body(())
            .unwrap();
        assert_eq!(ui_session_cookie_value(&req), Some("abc.def.ghi"));
    }

    #[test]
    fn missing_cookie_yields_none() {
        let req = HttpRequest::builder().body(()).unwrap();
        assert_eq!(ui_session_cookie_value(&req), None);
    }

    #[test]
    fn valid_express_session_jwt_is_accepted() {
        let token = signed_session(&ui_session_claims());
        assert!(is_valid_ui_session(&token, SECRET));
    }

    #[test]
    fn expired_session_jwt_is_rejected() {
        let claims = json!({
            "type": "ui-session",
            "iat": 1700000000_u64,
            "exp": (chrono::Utc::now().timestamp() - 10) as u64,
        });
        let token = signed_session(&claims);
        assert!(!is_valid_ui_session(&token, SECRET));
    }

    #[test]
    fn wrong_claim_type_is_rejected() {
        let claims = json!({
            "type": "other-session",
            "exp": (chrono::Utc::now().timestamp() + 3600) as u64,
        });
        let token = signed_session(&claims);
        assert!(!is_valid_ui_session(&token, SECRET));
    }

    #[test]
    fn foreign_secret_is_rejected() {
        let token = signed_session(&ui_session_claims());
        assert!(!is_valid_ui_session(&token, "attacker-secret"));
    }
}
