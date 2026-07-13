//! Health and capability reporting (tasks P0.2, P0.3).
//!
//! - `GET /api/health` — v2 server health, used by TUI to detect a
//!   dead kernel and offer reconnect.
//! - `GET /global/health` — single-instance health, alias used by some
//!   internal scripts.
//! - `GET /api/permission/saved` — saved permission query (MCP).

use axum::Json;
use serde_json::{json, Value};

pub async fn get_api_health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn get_global_health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "kind": "external-kernel",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn get_permission_saved() -> Json<Value> {
    Json(json!({ "data": [] }))
}
