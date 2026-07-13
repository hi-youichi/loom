//! Provider OAuth endpoints (task P2.22).

use axum::{extract::Path, Json};
use serde_json::{json, Value};

/// `POST /provider/auth` — initiate OAuth flow.
pub async fn post_provider_auth(Json(body): Json<Value>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "providerID": body.get("providerID").and_then(|v| v.as_str()).unwrap_or(""),
    }))
}

/// `GET /provider/auth/:id` — fetch auth state.
pub async fn get_provider_auth(Path(_id): Path<String>) -> Json<Value> {
    Json(json!({
        "data": { "status": "signed-out" }
    }))
}

/// `GET /api/provider/auth/:id` — v2 alias.
pub async fn get_api_provider_auth(Path(_id): Path<String>) -> Json<Value> {
    get_provider_auth(Path(_id)).await
}

/// `POST /api/provider/auth` — v2 alias.
pub async fn post_api_provider_auth(Json(body): Json<Value>) -> Json<Value> {
    post_provider_auth(Json(body)).await
}

/// `DELETE /api/provider/auth/:id` — sign out.
pub async fn delete_api_provider_auth(Path(_id): Path<String>) -> Json<Value> {
    Json(json!({ "ok": true }))
}
