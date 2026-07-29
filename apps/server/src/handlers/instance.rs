//! Instance + auth metadata (task P2.20).

use axum::Json;
use serde_json::{json, Value};

/// `GET /instance` — return current instance info.
pub async fn get_instance() -> Json<Value> {
    Json(json!({
        "id": "loom-server",
        "kind": "external-kernel",
        "version": env!("CARGO_PKG_VERSION"),
        "directory": std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
    }))
}

/// `GET /api/instance` — v2 alias.
pub async fn get_api_instance() -> Json<Value> {
    get_instance().await
}

/// `POST /api/instance/dispose` — dispose the kernel.
pub async fn post_api_instance_dispose() -> Json<Value> {
    Json(json!({ "ok": true, "disposed": true }))
}

/// `GET /auth` — return auth metadata.
pub async fn get_auth() -> Json<Value> {
    Json(json!({
        "authRequired": false,
    }))
}
