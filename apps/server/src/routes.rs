//! HTTP route registry — ACP-only server.

use axum::{middleware, routing::{get, post}, Router};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::auth::{log_authorization_header, require_valid_token};
use crate::handlers;
use crate::state::SharedState;

/// WebSocket paths that the Relay transport may connect to.
pub const RELAY_WEBSOCKET_ALLOWLIST: &[&str] = &["/acp"];

/// Build the application router.
pub fn build_router(state: SharedState) -> Router {
    Router::new()
        // ─── ACP WebSocket ───────────────────────────────────────
        .route("/acp", get(handlers::acp::connect))
        // ─── Health ──────────────────────────────────────────────
        .route("/api/health", get(handlers::health::get_api_health))
        .route("/global/health", get(handlers::health::get_global_health))
        .route("/metrics", get(handlers::health::get_acp_metrics))
        // ─── OpenChamber web compat (first takeover batch) ───────
        .route("/health", get(handlers::openchamber::get_health))
        .route("/api/fs/home", get(handlers::openchamber::get_fs_home))
        .route("/api/fs/list", get(handlers::openchamber::get_fs_list))
        .route("/api/path", get(handlers::openchamber::get_path))
        .route(
            "/api/project/current",
            get(handlers::openchamber::get_project_current),
        )
        .route("/api/session", get(handlers::openchamber::list_sessions))
        .route(
            "/api/session-folders",
            get(handlers::openchamber::get_session_folders),
        )
        .route(
            "/api/config/settings",
            get(handlers::openchamber::get_settings).put(handlers::openchamber::put_settings),
        )
        .route("/api/config/themes", get(handlers::openchamber::get_themes))
        .route("/auth/session", get(handlers::openchamber::get_auth_session))
        .route("/auth/url-token", post(handlers::openchamber::post_url_token))
        .route(
            "/auth/passkey/status",
            get(handlers::openchamber::get_passkey_status),
        )
        // ─── Auth middleware ─────────────────────────────────────
        .layer(middleware::from_fn(require_valid_token))
        .layer(middleware::from_fn(log_authorization_header))
        // ─── CORS ────────────────────────────────────────────────
        .layer(CorsLayer::very_permissive())
        // ─── Tracing ─────────────────────────────────────────────
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::RELAY_WEBSOCKET_ALLOWLIST;

    #[test]
    fn relay_websocket_allowlist_includes_acp() {
        assert!(RELAY_WEBSOCKET_ALLOWLIST.contains(&"/acp"));
    }
}
