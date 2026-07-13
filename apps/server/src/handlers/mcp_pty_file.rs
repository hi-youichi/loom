//! MCP, PTY, file, and find endpoint groups (task P2.20).

use axum::{
    extract::{Path, Query},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

// ───────────────────────── MCP ─────────────────────────

/// `GET /mcp` — v2 SDK path.
pub async fn get_mcp_status() -> Json<Value> {
    Json(json!({}))
}

/// `GET /mcp/status` — legacy pre-v2 path.
pub async fn get_mcp_status_legacy() -> Json<Value> {
    Json(json!({}))
}

/// `GET /api/mcp` — v2 alias.
pub async fn get_api_mcp_status() -> Json<Value> {
    Json(json!({"data": {}}))
}

/// `POST /mcp/:name/auth` — MCP server authentication stub.
pub async fn post_mcp_auth(Path(_name): Path<String>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `POST /api/mcp/:name/auth` — v2 alias.
pub async fn post_api_mcp_auth(Path(name): Path<String>) -> Json<Value> {
    post_mcp_auth(Path(name)).await
}

// ───────────────────────── PTY ─────────────────────────

/// `GET /pty` — list running PTY sessions.
pub async fn get_pty_list() -> Json<Value> {
    Json(json!([]))
}

/// `POST /pty` — create new PTY session.
pub async fn post_pty(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!({
        "id": format!("pty_{}", uuid::Uuid::new_v4().simple()),
    }))
}

/// `GET /api/pty` — v2 alias.
pub async fn get_api_pty_list() -> Json<Value> {
    Json(json!({"data": []}))
}

/// `POST /api/pty` — v2 alias.
pub async fn post_api_pty(Json(body): Json<Value>) -> Json<Value> {
    post_pty(Json(body)).await
}

/// `GET /pty/:id` — get single PTY.
pub async fn get_pty_one(Path(_id): Path<String>) -> Json<Value> {
    Json(json!({
        "id": _id,
    }))
}

/// `PATCH /pty/:id` — update PTY.
pub async fn patch_pty_one(Path(_id): Path<String>, Json(_body): Json<Value>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `DELETE /pty/:id` — drop PTY.
pub async fn delete_pty_one(Path(_id): Path<String>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

// ───────────────────────── File ─────────────────────────

/// `GET /file` — list `/file/:path` content.
pub async fn get_file(Query(_q): Query<FileQuery>) -> Json<Value> {
    Json(json!({ "content": "" }))
}

/// `PUT /file` — write content.
pub async fn put_file(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `GET /api/file` — v2 alias.
pub async fn get_api_file(Query(q): Query<FileQuery>) -> Json<Value> {
    get_file(Query(q)).await
}

#[derive(Deserialize, Default)]
pub struct FileQuery {
    #[serde(default)]
    pub path: Option<String>,
}

// ───────────────────────── Find ─────────────────────────

/// `GET /find` — text-based file query used by the current SDK.
pub async fn get_find(Query(_query): Query<FindQuery>) -> Json<Value> {
    Json(json!([]))
}

#[derive(Deserialize, Default)]
pub struct FindQuery {
    #[serde(default)]
    pub pattern: Option<String>,
}

/// `POST /find` — compatibility alias used by the rollout-v2 SDK.
pub async fn post_find(Json(body): Json<Value>) -> Json<Value> {
    Json(json!({
        "pattern": body.get("pattern").cloned().unwrap_or(json!(null)),
        "matches": [],
    }))
}

/// `POST /api/find` — v2 alias.
pub async fn post_api_find(Json(body): Json<Value>) -> Json<Value> {
    post_find(Json(body)).await
}

/// `GET /find/symbol` — symbol search.
pub async fn get_find_symbol() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `GET /api/find/symbol` — v2 alias.
pub async fn get_api_find_symbol() -> Json<Value> {
    get_find_symbol().await
}

/// `GET /find/file` — file glob.
pub async fn get_find_file() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `GET /api/find/file` — v2 alias.
pub async fn get_api_find_file() -> Json<Value> {
    get_find_file().await
}

/// Current SDK aliases.
pub async fn get_file_content(Query(query): Query<FileQuery>) -> Json<Value> {
    get_file(Query(query)).await
}

pub async fn get_file_status() -> Json<Value> {
    Json(json!([]))
}

pub async fn patch_mcp(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!(true))
}

pub async fn post_mcp_connect(Path(_name): Path<String>) -> Json<Value> {
    Json(json!(true))
}

pub async fn post_mcp_disconnect(Path(_name): Path<String>) -> Json<Value> {
    Json(json!(true))
}
