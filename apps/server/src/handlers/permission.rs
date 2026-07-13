//! Permission request handling (task P2.18).
//!
//! Most tools in Loom are pre-allowed in MVP; if a tool raises a
//! permission question, the TUI hits one of these endpoints to display
//! the prompt and let the user reply.

use axum::{extract::Path, http::StatusCode, Json};
use serde_json::{json, Value};

/// `POST /permission/:requestID/reply` — user answers a permission request.
pub async fn post_permission_reply(
    Path(request_id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    Json(json!({
        "ok": true,
        "requestID": request_id,
        "reply": body.get("reply").and_then(|v| v.as_str()).unwrap_or("deny"),
    }))
}

/// `POST /api/permission/:requestID/reply` — v2 alias.
pub async fn post_api_permission_reply(
    Path(request_id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    post_permission_reply(Path(request_id), Json(body)).await
}

pub async fn get_permission_pending() -> Json<Value> {
    Json(json!([]))
}

pub async fn get_api_permission_pending() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `POST /api/permission` — create a permission request (loom-server is
/// the one that raises it in MVP).
pub async fn post_api_permission(Json(_body): Json<Value>) -> (StatusCode, Json<Value>) {
    (StatusCode::CREATED, Json(json!({ "ok": true })))
}
