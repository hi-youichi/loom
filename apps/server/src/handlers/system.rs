//! System lifecycle HTTP routes migrated from the Express stack
//! (`/api/system/*`): info, free-port, probe-url, shutdown.

use std::net::TcpListener;

use axum::{extract::Query, response::IntoResponse, Json};
use serde_json::{json, Value};

pub async fn get_system_info() -> Json<Value> {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    let started_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
        .saturating_sub(start.elapsed().as_millis() as u64);
    Json(json!({
        "loomdeskVersion": env!("CARGO_PKG_VERSION"),
        "runtime": "loom",
        "pid": std::process::id(),
        "startedAt": started_ms,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    }))
}

#[derive(serde::Deserialize)]
pub struct FreePortQuery {
    #[serde(default)]
    pub host: Option<String>,
}

pub async fn get_free_port(Query(query): Query<FreePortQuery>) -> Json<Value> {
    let host = query.host.unwrap_or_else(|| "127.0.0.1".to_string());
    match TcpListener::bind((host.as_str(), 0)) {
        Ok(listener) => match listener.local_addr() {
            Ok(addr) => Json(json!({ "port": addr.port() })),
            Err(e) => Json(json!({ "error": e.to_string() })),
        },
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
pub struct ProbeUrlQuery {
    pub url: String,
}

pub async fn get_probe_url(Query(query): Query<ProbeUrlQuery>) -> impl IntoResponse {
    let parsed = match reqwest::Url::parse(&query.url) {
        Ok(url) => url,
        Err(e) => {
            return Json(json!({ "ok": false, "error": format!("invalid url: {e}") }));
        }
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Json(json!({ "ok": false, "error": "scheme must be http(s)" }));
    }
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            return Json(json!({ "ok": false, "error": e.to_string() }));
        }
    };
    match client
        .request(reqwest::Method::HEAD, parsed)
        .header("User-Agent", "loom-probe")
        .send()
        .await
    {
        Ok(response) => Json(json!({
            "ok": response.status().is_success(),
            "status": response.status().as_u16(),
        })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

pub async fn post_shutdown() -> Json<Value> {
    Json(json!({ "ok": true, "message": "loom server shutting down" }))
}
