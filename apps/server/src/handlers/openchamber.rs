//! OpenChamber web frontend compatibility routes (first takeover batch).
//!
//! The OpenChamber dev stack (vite dev server on :5180) proxies `/api`,
//! `/auth` and `/health` to its backend. Loom takes over that backend role;
//! this module implements the endpoints the web UI needs on first paint.
//!
//! Contracts were captured from the reference Express backend
//! (`openchamber-feat-dev/packages/web/server`).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

/// URL-token lifetime, mirroring the Express ui-auth default (10 minutes).
const URL_TOKEN_TTL_MS: i64 = 10 * 60 * 1000;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Resolve the requested directory, defaulting to the user home. `/`, ``
/// and missing values all fall back to home (matching Express behaviour
/// observed with `?directory=%2F`).
fn normalize_directory(params: &HashMap<String, String>) -> String {
    let raw = params.get("directory").map(|s| s.trim()).unwrap_or("");
    if raw.is_empty() || raw == "/" {
        home_dir().to_string_lossy().into_owned()
    } else {
        raw.to_string()
    }
}

fn project_id_for(directory: &str) -> String {
    if directory == "/" {
        "global".to_string()
    } else {
        Path::new(directory)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "global".to_string())
    }
}

/// `OPENCHAMBER_DATA_DIR` override, mirroring the Express layout
/// (`~/.config/openchamber` by default).
fn openchamber_data_dir() -> PathBuf {
    std::env::var_os("OPENCHAMBER_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config").join("openchamber"))
}

fn settings_file() -> PathBuf {
    openchamber_data_dir().join("settings.json")
}

fn read_settings_from(path: &Path) -> Value {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    }
}

fn write_settings_to(path: &Path, value: &Value) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(value)?;
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, &data)
        .and_then(|_| std::fs::rename(&tmp, path))
        .is_err()
    {
        // Windows rename over an existing file can fail; fall back to a
        // direct write (small file, acceptable tearing window).
        std::fs::write(path, data)?;
    }
    Ok(())
}

// ─── GET /health ──────────────────────────────────────────────────────

pub async fn get_health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "runtime": "loom",
        "compatibility": "opencode-v2",
    }))
}

// ─── GET /api/fs/home ─────────────────────────────────────────────────

pub async fn get_fs_home() -> Json<Value> {
    Json(json!({ "home": home_dir().to_string_lossy() }))
}

// ─── GET /api/fs/list?path= ───────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FsEntry {
    name: String,
    path: String,
    is_directory: bool,
    is_file: bool,
    is_symbolic_link: bool,
}

pub async fn get_fs_list(Query(params): Query<HashMap<String, String>>) -> impl IntoResponse {
    let raw = params.get("path").map(|s| s.trim()).unwrap_or("");
    let target = if raw.is_empty() || raw == "/" {
        home_dir()
    } else {
        PathBuf::from(raw)
    };

    let read = match std::fs::read_dir(&target) {
        Ok(read) => read,
        Err(err) => {
            let status = match err.kind() {
                io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
                io::ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            return (
                status,
                Json(json!({ "error": format!("failed to list directory: {err}") })),
            );
        }
    };

    let mut entries = Vec::new();
    for entry in read.flatten() {
        let Ok(file_type) = entry.file_type() else { continue };
        let is_symbolic_link = file_type.is_symlink();
        // Windows junctions/symlinks report is_dir() == false on the link
        // itself; follow the link so directory targets stay navigable.
        let is_directory = if is_symbolic_link {
            std::fs::metadata(entry.path())
                .map(|m| m.is_dir())
                .unwrap_or(false)
        } else {
            file_type.is_dir()
        };
        entries.push(FsEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path().to_string_lossy().into_owned(),
            is_directory,
            is_file: !is_directory,
            is_symbolic_link,
        });
    }

    (StatusCode::OK, Json(json!({ "entries": entries })))
}

// ─── GET /api/path?directory= ─────────────────────────────────────────

pub async fn get_path(Query(params): Query<HashMap<String, String>>) -> Json<Value> {
    let home = home_dir();
    let directory = normalize_directory(&params);
    let home_str = home.to_string_lossy();
    Json(json!({
        "home": home_str,
        "state": home.join(".local").join("state").join("opencode").to_string_lossy(),
        "config": home.join(".config").join("opencode").to_string_lossy(),
        "worktree": "/",
        "directory": directory,
    }))
}

// ─── GET /api/project/current?directory= ──────────────────────────────

pub async fn get_project_current(
    Query(params): Query<HashMap<String, String>>,
) -> Json<Value> {
    let raw = params.get("directory").map(|s| s.trim()).unwrap_or("");
    let is_root = raw.is_empty() || raw == "/";
    let directory = normalize_directory(&params);
    let ts = now_ms();
    Json(json!({
        "id": if is_root { "global".to_string() } else { project_id_for(&directory) },
        "worktree": "/",
        "time": { "created": ts, "updated": ts },
        "sandboxes": [],
    }))
}

// ─── GET /api/session?directory= ──────────────────────────────────────

pub async fn list_sessions(Query(_params): Query<HashMap<String, String>>) -> Json<Value> {
    // TODO(task): surface real Loom sessions (ACP hub persistence) in the
    // OpenChamber session shape. Empty list keeps first paint green.
    Json(Value::Array(Vec::new()))
}

// ─── GET /api/session-folders ─────────────────────────────────────────

pub async fn get_session_folders() -> Json<Value> {
    Json(json!({
        "version": 1,
        "foldersMap": {},
        "collapsedFolderIds": [],
        "updatedAt": now_ms(),
    }))
}

// ─── GET/PUT /api/config/settings ─────────────────────────────────────

pub async fn get_settings() -> Json<Value> {
    Json(read_settings_from(&settings_file()))
}

pub async fn put_settings(Json(payload): Json<Value>) -> impl IntoResponse {
    match write_settings_to(&settings_file(), &payload) {
        Ok(()) => (StatusCode::OK, Json(payload)),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("failed to persist settings: {err}") })),
        ),
    }
}

// ─── GET /api/config/themes ───────────────────────────────────────────

pub async fn get_themes() -> Json<Value> {
    // Custom theme registry is empty in Loom; built-in themes live in the
    // frontend bundle. Mirrors the Express default response.
    Json(json!({ "themes": [] }))
}

// ─── GET /auth/session ────────────────────────────────────────────────

pub async fn get_auth_session() -> Json<Value> {
    // Local dev mode: UI auth disabled (matches Express `disabled: true`).
    Json(json!({ "authenticated": true, "disabled": true }))
}

// ─── POST /auth/url-token ─────────────────────────────────────────────

pub async fn post_url_token() -> Json<Value> {
    let token = format!("oc_url_{}", uuid::Uuid::new_v4().simple());
    Json(json!({ "token": token, "expiresAt": now_ms() + URL_TOKEN_TTL_MS }))
}

// ─── GET /auth/passkey/status ─────────────────────────────────────────

pub async fn get_passkey_status() -> Json<Value> {
    Json(json!({
        "enabled": false,
        "hasPasskeys": false,
        "passkeyCount": 0,
        "rpID": null,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_directory_defaults_to_home() {
        let mut params = HashMap::new();
        assert!(!normalize_directory(&params).is_empty());
        params.insert("directory".to_string(), "/".to_string());
        assert!(!normalize_directory(&params).is_empty());
    }

    #[test]
    fn project_id_global_for_root() {
        assert_eq!(project_id_for("/"), "global");
        assert_eq!(project_id_for("C:\\Users\\heycj"), "heycj");
    }

    #[test]
    fn settings_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(read_settings_from(&path), json!({}));
        let value = json!({ "themeId": "flexoki-dark", "n": 1 });
        write_settings_to(&path, &value).unwrap();
        assert_eq!(read_settings_from(&path), value);
    }

    #[tokio::test]
    async fn health_reports_ok() {
        let Json(body) = get_health().await;
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn fs_list_returns_entries_and_missing_path_404() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("alpha")).unwrap();
        std::fs::write(dir.path().join("beta.txt"), b"x").unwrap();

        let mut params = HashMap::new();
        params.insert("path".to_string(), dir.path().to_string_lossy().into_owned());
        let response = get_fs_list(Query(params)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        let alpha = entries.iter().find(|e| e["name"] == "alpha").unwrap();
        assert_eq!(alpha["isDirectory"], true);
        assert_eq!(alpha["isFile"], false);

        let mut params = HashMap::new();
        params.insert("path".to_string(), dir.path().join("nope").to_string_lossy().into_owned());
        let response = get_fs_list(Query(params)).await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
