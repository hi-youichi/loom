//! HTTP route registry — ACP-only server.

use axum::{
    middleware,
    routing::get,
    Router,
};
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
