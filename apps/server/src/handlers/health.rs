//! Health and capability reporting (tasks P0.2, P0.3).
//!
//! - `GET /api/health` — v2 server health, used by TUI to detect a
//!   dead kernel and offer reconnect.
//! - `GET /global/health` — single-instance health, alias used by some
//!   internal scripts.
//! - `GET /api/permission/saved` — saved permission query (MCP).

use axum::{extract::State, response::IntoResponse, Json};
use serde_json::{json, Value};

use crate::state::SharedState;

pub async fn get_api_health() -> Json<Value> {
    // Contract (groups/health.ts:4-14): `GET /api/health` returns a closed
    // struct `Schema.Struct({ healthy: Schema.Literal(true) })` — exactly
    // `{ "healthy": true }`, unwrapped (NOT a Location.response). No query,
    // no payload, no extra fields. The only permitted success value is the
    // literal `true`.
    Json(json!({ "healthy": true }))
}

pub async fn get_global_health() -> Json<Value> {
    Json(json!({ "healthy": true }))
}

pub async fn get_permission_saved() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// Prometheus-compatible ACP runtime metrics. This endpoint intentionally
/// exposes only aggregate counters/gauges and never session ids or prompt data.
pub async fn get_acp_metrics(State(state): State<SharedState>) -> impl IntoResponse {
    let stats = state.acp_hub.stats().await;
    let body = format!(
        "# TYPE acp_active_connections gauge\nacp_active_connections {}\n\
# TYPE acp_active_sessions gauge\nacp_active_sessions {}\n\
# TYPE acp_active_prompts gauge\nacp_active_prompts {}\n\
# TYPE acp_total_prompts counter\nacp_total_prompts {}\n\
# TYPE acp_prompt_busy_total counter\nacp_prompt_busy_total {}\n\
# TYPE acp_notification_route_failures_total counter\nacp_notification_route_failures_total {}\n\
# TYPE acp_session_rebind_total counter\nacp_session_rebind_total {}\n\
# TYPE acp_connection_total counter\nacp_connection_total {}\n\
# TYPE acp_disconnect_total counter\nacp_disconnect_total {}\n",
        stats.active_connections,
        stats.active_sessions,
        stats.active_prompts,
        stats.total_prompts,
        stats.prompt_busy_rejections,
        stats.route_failures,
        stats.session_rebinds,
        stats.total_connections,
        stats.total_disconnects,
    );
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        body,
    )
}
