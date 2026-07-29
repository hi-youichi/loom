//! LSP and formatter status (tasks P0.2, P0.3).
//!
//! TUI's `sync.tsx:519-527` blocking `Promise.all` requires both
//! endpoints return a JSON envelope (default axum 404 has empty body
//! which `.json()` chokes on). We return empty arrays so any TUI build
//! can bootstrap without crashing. An empty array honestly means "no
//! language servers / formatters configured": no diagnostics, symbol
//! search, or formatting is ever performed by loom-server.

use axum::Json;
use serde_json::{json, Value};

/// `GET /lsp/status` — list of configured language servers.
///
/// Returns an empty array: no language server is wired, so there are no
/// diagnostics, symbols, or completions to report.
// Explicitly unsupported: no language server wired.
pub async fn get_lsp_status() -> Json<Value> {
    Json(json!([]))
}

/// `GET /formatter/status` — list of configured formatters.
///
/// Returns an empty array: no formatter is wired, so no formatting is
/// performed. There is no POST format route; status is the only surface
/// and it is honestly empty.
// Explicitly unsupported: no language server wired.
pub async fn get_formatter_status() -> Json<Value> {
    Json(json!([]))
}
