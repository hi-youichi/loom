//! HTTP route registry — ACP-only server.

use std::path::PathBuf;

use axum::{
    middleware,
    routing::{get, post, delete},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::auth::{log_authorization_header, require_valid_token};
use crate::handlers;
use crate::state::SharedState;
use crate::static_files;

/// WebSocket paths that the Relay transport may connect to.
pub const RELAY_WEBSOCKET_ALLOWLIST: &[&str] = &["/acp"];

/// Build the application router.
pub fn build_router(state: SharedState) -> Router {
    build_router_with_static(state, None)
}

/// Build the application router, optionally serving a built frontend from
/// `static_dir`.
///
/// Static assets bypass `LOOM_AUTH_TOKEN` enforcement so the browser can load
/// the app shell before any credentials exist (openchamber's public-shell
/// model); API routes keep full token enforcement.
pub fn build_router_with_static(state: SharedState, static_dir: Option<PathBuf>) -> Router {
    // Public auth routes (bypass token middleware)
    let auth_routes = Router::new()
        .route("/auth/session", get(handlers::session_auth::session_auth_status))
        .route("/auth/session", post(handlers::session_auth::session_auth_login))
        .with_state(state.clone());

    let api = Router::new()
        // ─── ACP WebSocket ───────────────────────────────────────
        .route("/acp", get(handlers::acp::connect))
        // ─── Health ──────────────────────────────────────────────
        .route("/api/health", get(handlers::health::get_api_health))
        .route("/global/health", get(handlers::health::get_global_health))
        .route("/metrics", get(handlers::health::get_acp_metrics))
        // ─── Session management (requires auth) ──────────────────
        .route("/auth/session", delete(handlers::session_auth::session_auth_logout))
        // ─── Auth middleware ─────────────────────────────────────
        .layer(middleware::from_fn(require_valid_token))
        .layer(middleware::from_fn(log_authorization_header))
        .with_state(state);

    let app = match static_dir {
        Some(dir) => api.merge(static_files::static_router(dir)).merge(auth_routes),
        None => api.merge(auth_routes),
    };

    // ─── CORS ────────────────────────────────────────────────
    app.layer(CorsLayer::very_permissive())
        // ─── Tracing ─────────────────────────────────────────────
        .layer(TraceLayer::new_for_http())
}

#[cfg(test)]
mod tests {
    use super::RELAY_WEBSOCKET_ALLOWLIST;

    #[test]
    fn relay_websocket_allowlist_includes_acp() {
        assert!(RELAY_WEBSOCKET_ALLOWLIST.contains(&"/acp"));
    }
}
