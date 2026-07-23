//! MCP, PTY, file, and find endpoint groups (task P2.20).
//!
//! - **MCP**: status returns a truthful empty server list; auth stubs return
//!   success shapes for compatibility. MCP connect/disconnect were removed
//!   in W4 (opencode has no mcp-connect group).
//! - **PTY**: contract-shaped `/api/pty*` routes (group-pty.ts). Real PTY
//!   sessions are backed by [`AppState::ptys`](crate::state::AppState);
//!   until the full PTY lifecycle (spawn/replay/WS-connect) ships in
//!   `handlers/pty.rs`, mutating endpoints return honest 501s and listing
//!   returns an empty result — never fake success.
//! - **File / Find**: real filesystem operations scoped to the project root
//!   with workspace-boundary enforcement (path-traversal protection).

use std::path::{Path as StdPath, PathBuf};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::SharedState;

// ───────────────────────── MCP ─────────────────────────

/// `GET /mcp` — v2 SDK path.
///
/// MCP server discovery from Loom config is not yet wired. Return a truthful
/// empty server list rather than a misleading `{}` or fake connected state.
pub async fn get_mcp_status() -> Json<Value> {
    Json(json!({ "servers": [] }))
}

/// `GET /mcp/status` — legacy pre-v2 path.
pub async fn get_mcp_status_legacy() -> Json<Value> {
    get_mcp_status().await
}

/// `GET /api/mcp` — v2 alias.
pub async fn get_api_mcp_status() -> Json<Value> {
    Json(json!({ "data": { "servers": [] } }))
}

/// `POST /mcp/:name/auth` — MCP server authentication stub.
pub async fn post_mcp_auth(Path(_name): Path<String>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `POST /api/mcp/:name/auth` — v2 alias.
pub async fn post_api_mcp_auth(Path(name): Path<String>) -> Json<Value> {
    post_mcp_auth(Path(name)).await
}

pub async fn patch_mcp(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!(true))
}

// ───────────────────────── PTY (group-pty.ts) ─────────────────────────
//
// Contract-shaped `/api/pty*` routes. Real PTY sessions are backed by
// `AppState::ptys`; until the full lifecycle (spawn/replay/WS-connect) lands
// in `handlers/pty.rs`, mutating endpoints return honest 501s and listing
// returns an empty result.

/// `GET /api/pty` — list PTY sessions (group-pty.ts `pty.list`).
/// Returns an honest empty result until the PTY registry is populated.
pub async fn get_api_pty_list() -> Json<Value> {
    Json(json!({"data": []}))
}

/// `POST /api/pty` — create PTY session (group-pty.ts `pty.create`).
/// Explicitly unsupported until PTY spawn is wired.
pub async fn post_api_pty(Json(_body): Json<Value>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({"error": "PTY create not implemented"})),
    )
}

/// `GET /api/pty/:ptyID` — get one PTY session (group-pty.ts `pty.get`).
/// Returns 404 because no PTY sessions exist yet.
pub async fn get_pty_one(Path(id): Path<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": format!("PTY '{}' not found", id)})),
    )
}

/// `PUT /api/pty/:ptyID` — update PTY title/size (group-pty.ts `pty.update`).
/// Explicitly unsupported until PTY lifecycle is wired.
pub async fn put_pty_one(
    Path(_id): Path<String>,
    Json(_body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({"error": "PTY update not implemented"})),
    )
}

/// `DELETE /api/pty/:ptyID` — remove PTY session (group-pty.ts `pty.remove`).
/// Returns 404 because no PTY sessions exist yet.
pub async fn delete_pty_one(Path(id): Path<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": format!("PTY '{}' not found", id)})),
    )
}

/// `POST /api/pty/:ptyID/connect-token` — create single-use connect ticket
/// (group-pty.ts `pty.connectToken`). Explicitly unsupported until PTY
/// lifecycle is wired.
pub async fn post_api_pty_connect_token(Path(_id): Path<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({"error": "PTY connect-token not implemented"})),
    )
}

/// `GET /api/pty/:ptyID/connect` — WebSocket connect (group-pty.ts
/// `pty.connect`). Requires a WebSocket upgrade which is not yet wired;
/// returns 426 Upgrade Required honestly.
pub async fn get_api_pty_connect(Path(_id): Path<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::UPGRADE_REQUIRED,
        Json(json!({"error": "PTY WebSocket connect requires upgrade; not yet implemented"})),
    )
}

// ───────────────────── workspace helpers ─────────────────────

/// Return the project root directory, canonicalized if it exists on disk.
fn project_root(state: &SharedState) -> PathBuf {
    let dir = state.project.read().directory.clone();
    let path = PathBuf::from(&dir);
    path.canonicalize()
        .unwrap_or_else(|_| normalize_path(&path))
}

/// Lexically normalize a path by resolving `.` and `..` components without
/// requiring the path to exist on disk (needed for write targets).
fn normalize_path(path: &StdPath) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

/// Resolve `requested` (absolute or relative to `root`) and verify it stays
/// inside `root`. Returns a 403 error response tuple on escape.
fn resolve_within(
    root: &StdPath,
    requested: Option<&str>,
) -> Result<PathBuf, (StatusCode, Json<Value>)> {
    let raw = requested.unwrap_or(".").trim();
    let requested_path = StdPath::new(raw);
    let absolute = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        root.join(requested_path)
    };
    let normalized = normalize_path(&absolute);
    if normalized.starts_with(root) {
        Ok(normalized)
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": format!("path '{}' escapes workspace boundary", raw)})),
        ))
    }
}

/// Convert a filesystem error into an appropriate status response.
fn fs_error_response(error: &std::io::Error) -> (StatusCode, Json<Value>) {
    let status = if error.kind() == std::io::ErrorKind::NotFound {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, Json(json!({"error": error.to_string()})))
}

/// Extract the trailing file-name component from a path string.
fn path_file_name(path: &str) -> String {
    StdPath::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Recursive directory walk that collects file paths whose name or relative
/// path contains `pattern` (case-insensitive). An empty pattern returns all
/// files. Skips hidden dirs and common noise directories; depth-limited.
fn walk_find(root: &StdPath, dir: &StdPath, pattern: &str, depth: u8) -> Vec<Value> {
    if depth > 10 {
        return Vec::new();
    }
    let mut results = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return results;
    };
    let needle = pattern.to_lowercase();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip hidden directories and common noise.
        if name.starts_with('.') || matches!(name.as_str(), "node_modules" | "target" | ".git") {
            continue;
        }
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        if needle.is_empty()
            || rel.to_lowercase().contains(&needle)
            || name.to_lowercase().contains(&needle)
        {
            results.push(json!({ "path": rel, "file": name }));
        }
        if path.is_dir() {
            results.extend(walk_find(root, &path, pattern, depth + 1));
        }
    }
    results
}

/// Search for files whose path/name contains `pattern` (case-insensitive).
/// Tries ripgrep first; falls back to a plain recursive walk.
fn find_files(root: &StdPath, pattern: &str) -> Vec<Value> {
    // Fast path: ripgrep for listing files matching the pattern.
    if !pattern.is_empty() {
        if let Ok(output) = std::process::Command::new("rg")
            .args(["--files", "--iglob", &format!("*{pattern}*")])
            .current_dir(root)
            .output()
        {
            if output.status.success() {
                return String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(|line| json!({"path": line, "file": path_file_name(line)}))
                    .collect();
            }
        }
    }
    // Fallback: recursive walk.
    walk_find(root, root, pattern, 0)
}

/// Recursive content search (like ripgrep) returning `LegacyMatch[]` entries.
/// Searches file contents line-by-line for `pattern` (case-insensitive).
fn grep_content(root: &StdPath, pattern: &str) -> Vec<Value> {
    fn grep_recursive(
        root: &StdPath,
        dir: &StdPath,
        needle: &str,
        results: &mut Vec<Value>,
        depth: u8,
    ) {
        if depth > 10 || results.len() > 500 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || matches!(name.as_str(), "node_modules" | "target" | ".git")
            {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                grep_recursive(root, &path, needle, results, depth + 1);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let needle_lower = needle.to_lowercase();
                let mut byte_offset = 0usize;
                for (line_idx, line) in content.split('\n').enumerate() {
                    let line_start = byte_offset;
                    let line_lower = line.to_lowercase();
                    if let Some(first_hit) = line_lower.find(&needle_lower) {
                        let mut submatches = Vec::new();
                        let mut search_from = 0usize;
                        while let Some(pos) = line_lower[search_from..].find(&needle_lower) {
                            let abs_start = search_from + pos;
                            let abs_end = abs_start + needle.len();
                            submatches.push(json!({
                                "match": &line[abs_start..abs_end.min(line.len())],
                                "start": abs_start,
                                "end": abs_end.min(line.len()),
                            }));
                            search_from = abs_end;
                            if search_from >= line.len() {
                                break;
                            }
                        }
                        results.push(json!({
                            "path": rel,
                            "lines": { "text": line },
                            "line_number": line_idx + 1,
                            "absolute_offset": line_start + first_hit,
                            "submatches": submatches,
                        }));
                    }
                    byte_offset += line.len() + 1;
                }
            }
        }
    }
    let mut results = Vec::new();
    grep_recursive(root, root, pattern, &mut results, 0);
    results
}

// ───────────────────────── File ─────────────────────────

/// `GET /file?path=...` — directory listing if path is a directory,
/// file content if path is a file. Directory listings return `LegacyEntry[]`.
pub async fn get_file(State(state): State<SharedState>, Query(q): Query<FileQuery>) -> Response {
    let root = project_root(&state);
    let path = match resolve_within(&root, q.path.as_deref()) {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    if path.is_dir() {
        let entries: Vec<Value> = match std::fs::read_dir(&path) {
            Ok(rd) => rd
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let meta = e.metadata().ok()?;
                    let abs = e.path();
                    let rel = abs
                        .strip_prefix(&root)
                        .unwrap_or(&abs)
                        .to_string_lossy()
                        .to_string();
                    let kind = if meta.is_dir() { "directory" } else { "file" };
                    Some(json!({
                        "name": name,
                        "path": rel,
                        "absolute": abs.to_string_lossy(),
                        "type": kind,
                        "ignored": false,
                    }))
                })
                .collect(),
            Err(e) => return fs_error_response(&e).into_response(),
        };
        return Json(json!(entries)).into_response();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => Json(json!({
            "content": content,
            "path": path.to_string_lossy(),
        }))
        .into_response(),
        Err(e) => fs_error_response(&e).into_response(),
    }
}

/// `PUT /file` — write content to a file within the workspace.
pub async fn put_file(State(state): State<SharedState>, Json(body): Json<Value>) -> Response {
    let Some(path_str) = body.get("path").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "path is required"})),
        )
            .into_response();
    };
    let content = body.get("content").and_then(Value::as_str).unwrap_or("");
    let root = project_root(&state);
    let path = match resolve_within(&root, Some(path_str)) {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    // Ensure parent directory exists for writes.
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return fs_error_response(&e).into_response();
            }
        }
    }
    match std::fs::write(&path, content) {
        Ok(()) => Json(json!({
            "ok": true,
            "path": path.to_string_lossy(),
        }))
        .into_response(),
        Err(e) => fs_error_response(&e).into_response(),
    }
}

/// `GET /api/file` — v2 alias.
pub async fn get_api_file(
    State(state): State<SharedState>,
    Query(q): Query<FileQuery>,
) -> Response {
    get_file(State(state), Query(q)).await
}

#[derive(Deserialize, Default)]
pub struct FileQuery {
    #[serde(default)]
    pub path: Option<String>,
}

/// Current SDK aliases — delegate to [`get_file`].
pub async fn get_file_content(
    State(state): State<SharedState>,
    Query(q): Query<FileQuery>,
) -> Response {
    get_file(State(state), Query(q)).await
}

pub async fn get_file_status() -> Json<Value> {
    Json(json!([]))
}

// ───────────────────────── Find ─────────────────────────

/// `GET /find?pattern=...` — content search (like ripgrep) returning
/// `LegacyMatch[]` entries within the workspace.
pub async fn get_find(
    State(state): State<SharedState>,
    Query(query): Query<FindQuery>,
) -> Response {
    let root = project_root(&state);
    let pattern = query.pattern.as_deref().unwrap_or("");
    if pattern.is_empty() {
        return Json(json!([])).into_response();
    }
    let matches = grep_content(&root, pattern);
    Json(json!(matches)).into_response()
}

#[derive(Deserialize, Default)]
pub struct FindQuery {
    #[serde(default)]
    pub pattern: Option<String>,
}

/// `POST /find` — compatibility alias used by the rollout-v2 SDK.
pub async fn post_find(State(state): State<SharedState>, Json(body): Json<Value>) -> Response {
    let root = project_root(&state);
    let pattern = body.get("pattern").and_then(Value::as_str).unwrap_or("");
    let matches = find_files(&root, pattern);
    Json(json!({
        "pattern": body.get("pattern").cloned().unwrap_or(json!(null)),
        "matches": matches,
    }))
    .into_response()
}

/// `POST /api/find` — v2 alias.
pub async fn post_api_find(State(state): State<SharedState>, Json(body): Json<Value>) -> Response {
    post_find(State(state), Json(body)).await
}

/// `GET /find/symbol` — symbol search (requires LSP; not wired).
pub async fn get_find_symbol() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `GET /api/find/symbol` — v2 alias.
pub async fn get_api_find_symbol() -> Json<Value> {
    get_find_symbol().await
}

/// `GET /find/file` — list file paths in the project directory (or a sub-path).
/// Returns bare `string[]` of relative file paths.
pub async fn get_find_file(
    State(state): State<SharedState>,
    Query(q): Query<FileQuery>,
) -> Response {
    let root = project_root(&state);
    let dir = match resolve_within(&root, q.path.as_deref()) {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    let files: Vec<String> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| {
                let abs = e.path();
                abs
                    .strip_prefix(&root)
                    .unwrap_or(&abs)
                    .to_string_lossy()
                    .to_string()
            })
            .collect(),
        Err(e) => return fs_error_response(&e).into_response(),
    };
    Json(json!(files)).into_response()
}

/// `GET /api/find/file` — v2 alias.
pub async fn get_api_find_file(
    State(state): State<SharedState>,
    Query(q): Query<FileQuery>,
) -> Response {
    get_find_file(State(state), Query(q)).await
}
