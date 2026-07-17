//! Experimental endpoint group (task P2.21).
//!
//! TUI hits `/experimental/*` for capability discovery and resource
//! listing. We return empty placeholders so the bootstrap resolves; a
//! follow-up PR can wire real sub-routes when loom has actual
//! corresponding features.
//!
//! ## Worktree lifecycle removed (W4 cleanup)
//!
//! opencode has NO worktree group. The previous worktree lifecycle
//! handlers (`/worktree`, `/experimental/worktree/*`) were removed in
//! task W4 along with their routes. This server reports the active
//! directory as informational metadata only — it does not manage a git
//! worktree lifecycle.

use axum::{extract::Path, Json};
use serde_json::{json, Value};

/// `GET /experimental/capabilities` — list enabled features.
pub async fn get_capabilities() -> Json<Value> {
    Json(json!({
        "backgroundSubagents": true,
        // Compatibility flags used by rollout-v2 clients.
        "agents": true,
        "tools": true,
        "mcp": true,
        "permissions": false,
        "questions": false,
        "sessions": true,
        "experimentalTools": false,
    }))
}

/// `GET /experimental/console` — event stream registration.
pub async fn get_console() -> Json<Value> {
    Json(json!({
        "data": {
            "endpoint": "/experimental/console",
        }
    }))
}

/// `GET /experimental/console/orgs` — org list (always empty in MVP).
pub async fn get_console_orgs() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `POST /experimental/console/org` — create an org (stub).
pub async fn post_console_org(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `GET /experimental/resource` — list resources.
pub async fn get_resource() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `GET /experimental/resource/list` — same.
pub async fn get_resource_list() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `POST /experimental/resource` — create a resource.
pub async fn post_resource(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `GET /experimental/resource/:id` — get one.
pub async fn get_resource_one(Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "id": id }))
}

/// `DELETE /experimental/resource/:id` — delete one.
pub async fn delete_resource_one(Path(_id): Path<String>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `POST /experimental/eval` — evaluate a snippet. Stub.
pub async fn post_eval(Json(body): Json<Value>) -> Json<Value> {
    Json(json!({ "result": body.get("input").cloned().unwrap_or(json!(null)) }))
}
