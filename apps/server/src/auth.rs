//! Authorization middleware.
//!
//! Three credential modes, combinable:
//!
//! 1. `ANUREO_AUTH_TOKEN` — programmatic `Authorization: Bearer <token>`.
//! 2. UI session JWTs (`oc_ui_session` cookie or `_anureo.dev/auth/*`
//!    pre-auth protocol messages). The HS256 secret is resolved from
//!    `ANUREO_JWT_SECRET`, `OPENCODE_JWT_SECRET`, or the shared anureo Desk
//!    data-dir file `<data-dir>/jwt-secret` (same file the Express ui-auth
//!    controller generates), so cookies minted by Express and tokens minted
//!    over the ACP pre-auth protocol verify interchangeably.
//! 3. `ANUREO_UI_PASSWORD` — password login over the `/acp` pre-auth
//!    protocol (`_anureo.dev/auth/login`), mirroring Express `POST
//!    /auth/session` (scrypt verify + per-IP rate limiting + JWT mint).
//!
//! When no token/secret/password is configured, development mode allows all
//! requests.

use axum::{
    extract::Request,
    http::{header::COOKIE, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub const AUTHORIZATION: &str = "authorization";

/// Environment variable holding the required bearer token. Unset/empty ⇒
/// not enforced. Set ⇒ requests must present a matching
/// `Authorization: Bearer <token>` header.
pub const AUTH_TOKEN_ENV: &str = "ANUREO_AUTH_TOKEN";

/// Environment variable holding the anureo Desk Express JWT secret (contents of
/// `<data-dir>/jwt-secret`). Unset/empty ⇒ cookie auth not enforced.
pub const JWT_SECRET_ENV: &str = "ANUREO_JWT_SECRET";

/// Legacy alias Express reads first for the JWT secret.
pub const JWT_SECRET_ENV_LEGACY: &str = "ANUREO_JWT_SECRET";

/// Environment variable holding the anureo Desk UI password (same variable the
/// Express server reads; `options.uiPassword` in Express maps to it).
pub const UI_PASSWORD_ENV: &str = "ANUREO_UI_PASSWORD";

/// Session cookie name minted by Express ui-auth (`SESSION_COOKIE_NAME`).
pub const UI_SESSION_COOKIE: &str = "oc_ui_session";

/// UI session TTL, matching Express `SESSION_TTL_MS`.
const SESSION_TTL_SECS: u64 = 12 * 60 * 60;
/// Trusted-device session TTL, matching Express `TRUSTED_DEVICE_SESSION_TTL_MS`.
const TRUSTED_DEVICE_TTL_SECS: u64 = 7 * 24 * 60 * 60;

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

/// The anureo Desk data dir shared with the Express server
/// (`ANUREO_DATA_DIR` or `~/.config/anureo`).
fn anureo_data_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("ANUREO_DATA_DIR") {
        if !dir.trim().is_empty() {
            return std::path::PathBuf::from(dir.trim());
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join(".config")
        .join("anureo")
}

/// Read the configured Express JWT secret: `ANUREO_JWT_SECRET` /
/// `OPENCODE_JWT_SECRET` env override first, then the shared data-dir file
/// the Express ui-auth controller generates (`<data-dir>/jwt-secret`, hex).
fn configured_jwt_secret() -> Option<String> {
    if let Some(secret) = std::env::var(JWT_SECRET_ENV)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
    {
        return Some(secret);
    }
    if let Some(secret) = std::env::var(JWT_SECRET_ENV_LEGACY)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
    {
        return Some(secret);
    }
    std::fs::read_to_string(anureo_data_dir().join("jwt-secret"))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Resolve the signing secret, generating and persisting a fresh one (same
/// format Express writes: 32 random bytes as hex, mode 0600) when none exists
/// yet. Called only on the mint path (password login); verification keeps
/// using [`configured_jwt_secret`] so a anureo-only install without logins
/// never materializes a secret file.
fn ensure_jwt_secret_for_minting() -> Option<String> {
    if let Some(secret) = configured_jwt_secret() {
        return Some(secret);
    }
    let secret = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let dir = anureo_data_dir();
    if let Err(error) =
        std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join("jwt-secret"), &secret))
    {
        tracing::warn!(%error, "failed to persist generated JWT secret");
        return None;
    }
    tracing::info!(dir = %dir.display(), "generated and persisted JWT secret");
    Some(secret)
}

/// Verify a login candidate against the configured UI password using scrypt
/// with a random per-process salt (same construction as Express `ui-auth`).
fn verify_ui_password(candidate: &str) -> bool {
    let Some(expected) = configured_ui_password() else {
        return false;
    };
    use scrypt::{scrypt, Params};
    let params = Params::new(14, 8, 1, 64).expect("scrypt params");
    // Two UUIDv4s = 32 bytes of crypto randomness in hex; uuid is already a
    // workspace dependency and this is only salt material.
    let salt = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
    .into_bytes();
    let mut expected_hash = [0u8; 64];
    if scrypt(expected.as_bytes(), &salt, &params, &mut expected_hash).is_err() {
        return false;
    }
    let mut candidate_hash = [0u8; 64];
    if scrypt(
        candidate.trim().as_bytes(),
        &salt,
        &params,
        &mut candidate_hash,
    )
    .is_err()
    {
        return false;
    }
    token_eq(&expected_hash, &candidate_hash)
}

/// Read the configured UI password from the environment, if any.
fn configured_ui_password() -> Option<String> {
    std::env::var(UI_PASSWORD_ENV)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Whether the UI password gate is configured (drives `auth/status` and the
/// availability of `_anureo.dev/auth/login`).
pub fn ui_password_configured() -> bool {
    configured_ui_password().is_some()
}

/// Mint a UI session JWT (HS256, `type: ui-session`) with the same claim shape
/// the Express controller emits (`jose` `.setIssuedAt().setExpirationTime`).
fn mint_ui_session_jwt(secret: &str, ttl_secs: u64) -> Option<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = serde_json::json!({
        "type": "ui-session",
        "iat": now,
        "exp": now + ttl_secs as i64,
    });
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .ok()
}

/// Outcome of a pre-auth password login attempt.
pub struct LoginOutcome {
    pub ok: bool,
    pub session_token: Option<String>,
    pub expires_at_unix: Option<u64>,
    pub retry_after_secs: Option<u64>,
}

/// Attempt a UI password login over the pre-auth protocol: rate limit →
/// scrypt verify → mint JWT. Mirrors Express `handleSessionCreate`.
pub fn attempt_ui_login(password: &str, trust_device: bool, peer_ip: &str) -> LoginOutcome {
    if !login_rate_allow(peer_ip) {
        return LoginOutcome {
            ok: false,
            session_token: None,
            expires_at_unix: None,
            retry_after_secs: Some(login_rate_retry_after(peer_ip)),
        };
    }
    if !verify_ui_password(password) {
        login_rate_record_failure(peer_ip);
        return LoginOutcome {
            ok: false,
            session_token: None,
            expires_at_unix: None,
            retry_after_secs: None,
        };
    }
    login_rate_clear(peer_ip);
    let ttl = if trust_device {
        TRUSTED_DEVICE_TTL_SECS
    } else {
        SESSION_TTL_SECS
    };
    let Some(secret) = ensure_jwt_secret_for_minting() else {
        tracing::error!("password login succeeded but no JWT secret available");
        return LoginOutcome {
            ok: false,
            session_token: None,
            expires_at_unix: None,
            retry_after_secs: None,
        };
    };
    let token = mint_ui_session_jwt(&secret, ttl);
    LoginOutcome {
        ok: token.is_some(),
        session_token: token,
        expires_at_unix: Some((chrono::Utc::now().timestamp() as u64) + ttl),
        retry_after_secs: None,
    }
}

/// Verify a session token minted over the pre-auth protocol (or by Express).
pub fn ui_session_token_valid(token: &str) -> bool {
    configured_jwt_secret()
        .map(|secret| is_valid_ui_session(token, &secret))
        .unwrap_or(false)
}

// ─── Login rate limiting (per-IP, mirrors Express ui-auth) ────────────────

const RATE_LIMIT_WINDOW_MS: u64 = 5 * 60 * 1000;
const RATE_LIMIT_LOCKOUT_MS: u64 = 15 * 60 * 1000;
const RATE_LIMIT_CLEANUP_MS: u64 = 60 * 60 * 1000;
const RATE_LIMIT_DEFAULT_MAX_ATTEMPTS: u32 = 10;

#[derive(Clone, Debug)]
struct RateRecord {
    count: u32,
    last_attempt_ms: u64,
    locked_until_ms: Option<u64>,
}

static LOGIN_RATE_LIMITS: std::sync::LazyLock<
    parking_lot::Mutex<std::collections::HashMap<String, RateRecord>>,
> = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn login_rate_max_attempts() -> u32 {
    std::env::var("ANUREO_RATE_LIMIT_MAX_ATTEMPTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(RATE_LIMIT_DEFAULT_MAX_ATTEMPTS)
}

fn login_rate_allow(peer_ip: &str) -> bool {
    let mut limits = LOGIN_RATE_LIMITS.lock();
    sweep_stale_records(&mut limits);
    let now = now_ms();
    let Some(record) = limits.get(peer_ip) else {
        return true;
    };
    if let Some(locked_until) = record.locked_until_ms {
        if now < locked_until {
            return false;
        }
        limits.remove(peer_ip);
        return true;
    }
    now.saturating_sub(record.last_attempt_ms) > RATE_LIMIT_WINDOW_MS
        || record.count < login_rate_max_attempts()
}

fn login_rate_retry_after(peer_ip: &str) -> u64 {
    let limits = LOGIN_RATE_LIMITS.lock();
    limits
        .get(peer_ip)
        .and_then(|r| r.locked_until_ms)
        .map(|until| until.saturating_sub(now_ms()).div_ceil(1000))
        .filter(|secs| *secs > 0)
        .unwrap_or(RATE_LIMIT_LOCKOUT_MS.div_ceil(1000))
}

fn login_rate_record_failure(peer_ip: &str) {
    let mut limits = LOGIN_RATE_LIMITS.lock();
    let now = now_ms();
    let max_attempts = login_rate_max_attempts();
    let entry = limits.entry(peer_ip.to_string()).or_insert(RateRecord {
        count: 0,
        last_attempt_ms: now,
        locked_until_ms: None,
    });
    if now.saturating_sub(entry.last_attempt_ms) > RATE_LIMIT_WINDOW_MS {
        entry.count = 0;
    }
    entry.count += 1;
    entry.last_attempt_ms = now;
    if entry.count > max_attempts {
        entry.locked_until_ms = Some(now + RATE_LIMIT_LOCKOUT_MS);
    }
}

fn login_rate_clear(peer_ip: &str) {
    LOGIN_RATE_LIMITS.lock().remove(peer_ip);
}

fn sweep_stale_records(limits: &mut std::collections::HashMap<String, RateRecord>) {
    let now = now_ms();
    limits.retain(|_, record| {
        let expired = record.locked_until_ms.is_some_and(|until| now >= until);
        let stale = now.saturating_sub(record.last_attempt_ms) > RATE_LIMIT_CLEANUP_MS;
        !expired && !stale
    });
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
fn ui_session_cookie_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())?
        .split(';')
        .filter_map(|part| {
            part.trim()
                .strip_prefix(UI_SESSION_COOKIE)
                .and_then(|rest| rest.strip_prefix('='))
        })
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
        Ok(data) => data.claims.get("type").and_then(|claim| claim.as_str()) == Some("ui-session"),
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

/// Evaluate credentials from request headers alone. Same rules as
/// [`require_valid_token`]: dev mode (nothing configured) allows all;
/// otherwise a matching bearer token or a valid UI session cookie is
/// required. Used both by the HTTP middleware and by the `/acp` WebSocket
/// pre-auth gate, which cannot surface an HTTP 401 to browser clients
/// (the WebSocket API hides upgrade status codes).
pub fn credentials_valid(headers: &HeaderMap) -> bool {
    let anureo_token = configured_token();
    let jwt_secret = configured_jwt_secret();
    let ui_password = configured_ui_password();

    // Development mode: nothing gates the gateway.
    if anureo_token.is_none() && jwt_secret.is_none() && ui_password.is_none() {
        return true;
    }

    if let Some(expected) = &anureo_token {
        let header = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());
        if let Some(provided) = bearer_from_header(header) {
            if token_eq(provided.as_bytes(), expected.as_bytes()) {
                return true;
            }
        }
    }

    if let Some(secret) = &jwt_secret {
        if let Some(cookie_value) = ui_session_cookie_from_headers(headers) {
            if is_valid_ui_session(cookie_value, secret) {
                return true;
            }
        }
    }

    // Password gate configured (and no credential presented above): the
    // socket enters the pre-auth handshake and must login via
    // `_anureo.dev/auth/login` to mint a session token.
    false
}

/// Auth enforcement.
///
/// When neither `ANUREO_AUTH_TOKEN` nor `ANUREO_JWT_SECRET` is set →
/// development mode (allow all). Otherwise a request must present either a
/// matching Bearer token or a valid `oc_ui_session` session cookie.
pub async fn require_valid_token(req: Request, next: Next) -> Response {
    if credentials_valid(req.headers()) {
        return next.run(req).await;
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "missing or invalid authorization token" })),
    )
        .into_response()
}

/// Per-connection credential verdict stamped onto `/acp` upgrade requests by
/// [`record_acp_auth_verdict`]. `true` = authenticated (or development mode).
#[derive(Clone, Copy, Debug)]
pub struct AcpAuthVerdict(pub bool);

/// `/acp` auth middleware: instead of rejecting unauthenticated upgrades with
/// an HTTP 401 (invisible to browser WebSocket clients), the upgrade is
/// allowed and the verdict travels in the request extensions. The handler
/// then runs the pre-auth handshake for unauthenticated sockets (see
/// `handlers::acp::handle_pre_auth_socket`).
pub async fn record_acp_auth_verdict(mut req: Request, next: Next) -> Response {
    let verdict = AcpAuthVerdict(credentials_valid(req.headers()));
    req.extensions_mut().insert(verdict);
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

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
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            "other=1; oc_ui_session=abc.def.ghi; more=2"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            ui_session_cookie_from_headers(&headers),
            Some("abc.def.ghi")
        );
    }

    #[test]
    fn missing_cookie_yields_none() {
        assert_eq!(ui_session_cookie_from_headers(&HeaderMap::new()), None);
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
