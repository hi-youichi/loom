//! TUI control endpoints (task P1.16).
//!
//! The opencode TUI talks to the kernel on `/tui/*` to coordinate
//! running commands (e.g. preserve a "model picker" modal alive).
//! loom-server in MVP only registers trivial acknowledgements so a TUI
//! bootstrap doesn't fail when these endpoints appear in its spec list.

use axum::{extract::Path, Json};
use serde_json::{json, Value};

/// `POST /tui/command` — submit a TUI-side command.
pub async fn post_tui_command(Json(body): Json<Value>) -> Json<Value> {
    Json(json!({ "ok": true, "echo": body }))
}

/// `POST /tui/control/next` — wake the TUI's event loop.
pub async fn post_tui_control_next() -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `POST /tui/control/exit` — graceful shutdown. Returns quickly.
pub async fn post_tui_control_exit() -> Json<Value> {
    // Note: real exit happens on the *client* side. The kernel doesn't
    // own the TUI process, it just acknowledges.
    Json(json!({ "ok": true, "shutdown": true }))
}

/// `POST /tui/control/cancel/{request_id}` — cancel a TUI request.
pub async fn post_tui_control_cancel(Path(request_id): Path<String>) -> Json<Value> {
    Json(json!({ "ok": true, "cancelled": request_id }))
}

/// `POST /control/next` — global equivalent.
pub async fn post_control_next() -> Json<Value> {
    post_tui_control_next().await
}
