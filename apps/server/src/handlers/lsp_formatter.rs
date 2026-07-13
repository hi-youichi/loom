//! LSP and formatter status (tasks P0.2, P0.3).
//!
//! TUI's `sync.tsx:519-527` blocking `Promise.all` requires both
//! endpoints return a JSON envelope (default axum 404 has empty body
//! which `.json()` chokes on). We return `{ data: [] }` so any TUI
//! build can bootstrap without crashing.

use axum::Json;
use serde_json::{json, Value};

/// `GET /lsp/status`
pub async fn get_lsp_status() -> Json<Value> {
    Json(json!([]))
}

/// `GET /formatter/status`
pub async fn get_formatter_status() -> Json<Value> {
    Json(json!([]))
}
