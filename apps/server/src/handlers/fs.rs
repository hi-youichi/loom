//! HTTP handlers for the `server.fs` group (`groups/fs.ts`): location-scoped
//! filesystem routes resolved against the opencode `Location` contract.
//!
//! Endpoints (all `GET`, all accept the deepObject `LocationQuery`):
//!
//! | Endpoint              | Contract success                              |
//! |-----------------------|-----------------------------------------------|
//! | `GET /api/fs/read/*`  | raw bytes, `200 application/octet-stream`     |
//! | `GET /api/fs/list`    | `Location.response(FileSystem.Entry[])`       |
//! | `GET /api/fs/find`    | `Location.response(FileSystem.Entry[])`       |
//!
//! `FileSystem.Entry` = `{ path: RelativePath, type: "file" \| "directory" }`
//! (schema/filesystem.ts:14-18). `RelativePath` is a path relative to the
//! resolved location directory.
//!
//! All path inputs are resolved against the location directory and checked to
//! remain within it (path-traversal / escape protection), reusing the same
//! lexical normalization approach as the legacy `/file` handler (LS-016).
//! `read` serves raw bytes with `application/octet-stream` and 404 on absence;
//! `list`/`find` return real entries wrapped in the `Location.response`
//! envelope via [`location_response`](crate::location::location_response).

use std::path::{Component, Path as StdPath, PathBuf};

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::location::{location_response, LocationQuery};
use crate::state::SharedState;

// ───────────────────────── shared helpers ─────────────────────────

/// Resolve the location directory the request operates on: the
/// `?location[directory]=` value if supplied, otherwise the server's active
/// project directory (canonicalized when it exists on disk).
fn location_root(state: &SharedState, loc: &LocationQuery) -> PathBuf {
    let dir = loc.resolve_directory(state);
    let path = PathBuf::from(&dir);
    path.canonicalize().unwrap_or_else(|_| normalize(&path))
}

/// Lexically normalize a path by resolving `.`/`..` without requiring it to
/// exist on disk (needed so write/read targets can be validated before IO).
fn normalize(path: &StdPath) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

/// Resolve `requested` (absolute, or relative to `root`) and verify it stays
/// inside `root`. Returns a 403 response tuple on escape.
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
    let normalized = normalize(&absolute);
    if normalized.starts_with(root) {
        Ok(normalized)
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": format!("path '{}' escapes location boundary", raw)})),
        ))
    }
}

/// The relative-to-`root` rendering of `path` as a forward-slash path string,
/// matching `RelativePath` (schema/schema.ts). Falls back to the display path
/// if `path` is not under `root`.
fn relative_to(root: &StdPath, path: &StdPath) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

/// `FileSystem.Entry` = `{ path: RelativePath, type: "file" | "directory" }`
/// (schema/filesystem.ts:14-18).
#[derive(Serialize)]
struct FsEntry<'a> {
    path: String,
    #[serde(rename = "type")]
    kind: &'a str,
}

// ───────────────────────── fs.read ─────────────────────────

/// `GET /api/fs/read/*path` (groups/fs.ts:21-34) — serve one file relative to
/// the requested location as raw bytes.
///
/// - `200 application/octet-stream` with the file body on success.
/// - `404` when the file does not exist.
/// - `403` when the resolved path escapes the location boundary.
/// - `400` when the path maps to a directory (read serves files only).
///
/// Streams the raw bytes (matches `Schema.Uint8Array` via
/// `HttpApiSchema.asUint8Array()`), so binary files are served verbatim — no
/// JSON wrapping, no text re-encoding.
pub async fn read(
    State(state): State<SharedState>,
    loc: LocationQuery,
    Path(path): Path<String>,
) -> Response {
    let root = location_root(&state, &loc);
    let resolved = match resolve_within(&root, Some(&path)) {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    let meta = match std::fs::metadata(&resolved) {
        Ok(m) => m,
        Err(e) => {
            let status = if e.kind() == std::io::ErrorKind::NotFound {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return (status, Json(json!({"error": e.to_string()}))).into_response();
        }
    };
    if meta.is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "path is a directory, not a file"})),
        )
            .into_response();
    }
    let bytes = match std::fs::read(&resolved) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/octet-stream")],
        bytes,
    )
        .into_response()
}

// ───────────────────────── fs.list ─────────────────────────

/// `ListQuery` (groups/fs.ts:8-11): the deepObject `LocationQuery` fields plus
/// an optional `path: RelativePath` naming a sub-directory of the location to
/// list (defaults to the location root).
#[derive(Deserialize, Default)]
pub struct ListQuery {
    #[serde(flatten)]
    pub location: LocationQuery,
    #[serde(default)]
    pub path: Option<String>,
}

/// `GET /api/fs/list` (groups/fs.ts:35-48) — list the direct children of one
/// directory relative to the requested location.
///
/// Success: `Location.response(FileSystem.Entry[])` — `{ location, data: [
/// {path, type} ] }`. Returns a real `read_dir` listing (never an empty stub);
/// a non-existent directory yields 404, an escaping path yields 403.
pub async fn list(
    State(state): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> Response {
    let root = location_root(&state, &q.location);
    let dir = match resolve_within(&root, q.path.as_deref()) {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) => {
            let status = if e.kind() == std::io::ErrorKind::NotFound {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return (status, Json(json!({"error": e.to_string()}))).into_response();
        }
    };
    let mut entries: Vec<FsEntry> = Vec::new();
    for entry in rd.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let kind = if meta.is_dir() { "directory" } else { "file" };
        entries.push(FsEntry {
            path: relative_to(&root, &entry.path()),
            kind,
        });
    }
    // Stable, deterministic ordering for clients.
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    location_response(&state, entries).into_response()
}

// ───────────────────────── fs.write ─────────────────────────

#[derive(Deserialize)]
pub struct WriteBody {
    pub path: String,
    pub content: String,
}

pub async fn write(
    State(state): State<SharedState>,
    Query(loc): Query<LocationQuery>,
    Json(body): Json<WriteBody>,
) -> Response {
    let root = location_root(&state, &loc);
    let resolved = match resolve_within(&root, Some(&body.path)) {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    if let Some(parent) = resolved.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }
    match std::fs::write(&resolved, &body.content) {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"path": relative_to(&root, &resolved)})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ───────────────────────── fs.delete ─────────────────────────

#[derive(Deserialize)]
pub struct DeleteBody {
    pub path: String,
}

pub async fn delete(
    State(state): State<SharedState>,
    Query(loc): Query<LocationQuery>,
    Json(body): Json<DeleteBody>,
) -> Response {
    let root = location_root(&state, &loc);
    let resolved = match resolve_within(&root, Some(&body.path)) {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    let meta = match std::fs::metadata(&resolved) {
        Ok(m) => m,
        Err(e) => {
            let status = if e.kind() == std::io::ErrorKind::NotFound {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return (status, Json(json!({"error": e.to_string()}))).into_response();
        }
    };
    let result = if meta.is_dir() {
        std::fs::remove_dir_all(&resolved)
    } else {
        std::fs::remove_file(&resolved)
    };
    match result {
        Ok(_) => (StatusCode::OK, Json(json!({"path": relative_to(&root, &resolved)}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ───────────────────────── fs.rename ─────────────────────────

#[derive(Deserialize)]
pub struct RenameBody {
    pub from: String,
    pub to: String,
}

pub async fn rename(
    State(state): State<SharedState>,
    Query(loc): Query<LocationQuery>,
    Json(body): Json<RenameBody>,
) -> Response {
    let root = location_root(&state, &loc);
    let from = match resolve_within(&root, Some(&body.from)) {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    let to = match resolve_within(&root, Some(&body.to)) {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    if let Some(parent) = to.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }
    match std::fs::rename(&from, &to) {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"from": relative_to(&root, &from), "to": relative_to(&root, &to)})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ───────────────────────── fs.mkdir ─────────────────────────

#[derive(Deserialize)]
pub struct MkdirBody {
    pub path: String,
}

pub async fn mkdir(
    State(state): State<SharedState>,
    Query(loc): Query<LocationQuery>,
    Json(body): Json<MkdirBody>,
) -> Response {
    let root = location_root(&state, &loc);
    let resolved = match resolve_within(&root, Some(&body.path)) {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    match std::fs::create_dir_all(&resolved) {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"path": relative_to(&root, &resolved)})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ───────────────────────── fs.stat ─────────────────────────

#[derive(Deserialize, Default)]
pub struct StatQuery {
    #[serde(flatten)]
    pub location: LocationQuery,
    #[serde(default)]
    pub path: Option<String>,
}

pub async fn stat(
    State(state): State<SharedState>,
    Query(q): Query<StatQuery>,
) -> Response {
    let root = location_root(&state, &q.location);
    let resolved = match resolve_within(&root, q.path.as_deref()) {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    let meta = match std::fs::metadata(&resolved) {
        Ok(m) => m,
        Err(e) => {
            let status = if e.kind() == std::io::ErrorKind::NotFound {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return (status, Json(json!({"error": e.to_string()}))).into_response();
        }
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Json(json!({
        "path": relative_to(&root, &resolved),
        "size": meta.len(),
        "isDir": meta.is_dir(),
        "modified": modified,
    }))
    .into_response()
}

// ───────────────────────── fs.find ─────────────────────────

/// `FindQuery` (groups/fs.ts:13-18): deepObject `LocationQuery` plus the
/// `FileSystem.FindInput` search fields — required `query`, optional `type`
/// filter, optional `limit`.
#[derive(Deserialize, Default)]
pub struct FindQuery {
    #[serde(flatten)]
    pub location: LocationQuery,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// `GET /api/fs/find` (groups/fs.ts:49-62) — recursively find ranked
/// filesystem entries relative to the requested location whose path/name
/// contains the query (case-insensitive substring).
///
/// Success: `Location.response(FileSystem.Entry[])`. `query` is required by the
/// contract (`FileSystem.FindInput.query`); an absent query returns 400.
/// `type` filters to `"file"`/`"directory"`; `limit` caps the result count.
///
/// Uses ripgrep (`rg --files`) when available for speed, with a recursive walk
/// fallback — the same search strategy as the legacy `/find` handler.
pub async fn find(
    State(state): State<SharedState>,
    Query(q): Query<FindQuery>,
) -> Response {
    let Some(query) = q.query.as_deref() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "query is required"})),
        )
            .into_response();
    };
    if query.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "query must not be empty"})),
        )
            .into_response();
    }
    let root = location_root(&state, &q.location);
    let kind_filter = q.kind.as_deref();
    let limit = q.limit.unwrap_or(DEFAULT_FIND_LIMIT);

    let mut entries = find_entries(&root, query, kind_filter);
    entries.truncate(limit);
    location_response(&state, entries).into_response()
}

/// Default cap on the number of find results when the client omits `limit`.
/// Matches the legacy `/find` handler's practical bound.
const DEFAULT_FIND_LIMIT: usize = 200;

/// Recursively collect `FileSystem.Entry` objects under `root` whose relative
/// path or file name contains `needle` (case-insensitive). `kind_filter`, when
/// `Some`, restricts results to `"file"` or `"directory"`. Tries ripgrep first
/// for files, then falls back to a depth-limited walk.
fn find_entries(root: &StdPath, needle: &str, kind_filter: Option<&str>) -> Vec<FsEntry<'static>> {
    let lc = needle.to_lowercase();
    let want_files = kind_filter.map(|k| k == "file").unwrap_or(true);
    let want_dirs = kind_filter.map(|k| k == "directory").unwrap_or(true);

    // Fast path: ripgrep file listing for a file/any search.
    if want_files {
        if let Some(files) = find_via_ripgrep(root, &lc) {
            return files
                .into_iter()
                .map(|path| FsEntry {
                    path,
                    kind: "file",
                })
                .collect();
        }
    }

    // Fallback: recursive walk covering both files and directories.
    walk_find(root, root, &lc, 0, want_files, want_dirs)
}

/// Attempt a ripgrep file listing filtered to names containing `needle_lc`.
/// Returns `None` if ripgrep is unavailable or fails (caller falls back).
fn find_via_ripgrep(root: &StdPath, needle_lc: &str) -> Option<Vec<String>> {
    let output = std::process::Command::new("rg")
        .args(["--files", "--iglob", &format!("*{needle_lc}*")])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let rels: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.replace('\\', "/"))
        .collect();
    Some(rels)
}

/// Recursive directory walk collecting matching entries. Skips hidden entries
/// and common noise directories; depth-limited to bound work.
fn walk_find(
    root: &StdPath,
    dir: &StdPath,
    needle_lc: &str,
    depth: u8,
    want_files: bool,
    want_dirs: bool,
) -> Vec<FsEntry<'static>> {
    if depth > 10 {
        return Vec::new();
    }
    let mut results = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return results;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || matches!(name.as_str(), "node_modules" | "target" | ".git") {
            continue;
        }
        let path = entry.path();
        let is_dir = path.is_dir();
        let rel = relative_to(root, &path);
        let matches = rel.to_lowercase().contains(needle_lc) || name.to_lowercase().contains(needle_lc);
        if matches {
            let kind = if is_dir { "directory" } else { "file" };
            let wanted = (is_dir && want_dirs) || (!is_dir && want_files);
            if wanted {
                results.push(FsEntry {
                    path: rel,
                    kind,
                });
            }
        }
        if is_dir {
            results.extend(walk_find(
                root,
                &path,
                needle_lc,
                depth + 1,
                want_files,
                want_dirs,
            ));
        }
    }
    results
}
