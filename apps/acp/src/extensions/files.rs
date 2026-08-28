use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ignore::WalkBuilder;
use serde::Deserialize;
use serde_json::Value;

use super::auth;
use super::pagination::PaginatedResult;
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const FORBIDDEN_METHODS: &[&str] = &["read"];

const MAX_BINARY_READ_SIZE: u64 = 10 * 1024 * 1024;
const MAX_DOWNLOAD_SIZE: u64 = 10 * 1024 * 1024;
const MAX_TEXT_READ_SIZE: u64 = 1024 * 1024;
const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 1000;
const DEFAULT_EXEC_TIMEOUT_MS: u64 = 120_000;

pub struct FilesHandler;

impl FilesHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FilesHandler {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_path_string(path: &str) -> Result<(), ExtensionError> {
    if path.is_empty() {
        return Err(ExtensionError::invalid_params("path must not be empty"));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(ExtensionError::invalid_params(
            "absolute paths are not allowed",
        ));
    }
    if path.len() >= 2 && path.as_bytes()[1] == b':' {
        return Err(ExtensionError::invalid_params(
            "absolute paths are not allowed",
        ));
    }
    for segment in path.split(['/', '\\']) {
        if segment == ".." {
            return Err(ExtensionError::invalid_params(
                "path traversal (..) is not allowed",
            ));
        }
    }
    Ok(())
}

fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut result = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    result
}

fn resolve_path(path: &str, working_directory: Option<&Path>) -> Result<PathBuf, ExtensionError> {
    validate_path_string(path)?;
    let wd = working_directory
        .ok_or_else(|| ExtensionError::invalid_params("no working directory set"))?;
    let normalized = normalize_path(&wd.join(path));
    let wd_norm = normalize_path(wd);
    if !normalized.starts_with(&wd_norm) {
        return Err(ExtensionError::invalid_params(
            "path outside working directory",
        ));
    }
    Ok(normalized)
}

/// Resolve the `cwd` for `files/exec_commands`. Unlike file paths (always
/// relative, sandboxed via `resolve_path`), the exec cwd may legitimately be
/// absolute - the frontend sends the project directory verbatim (e.g.
/// `C:/repo/dir` from its directory store). Accept both forms, but keep the
/// sandbox invariant: the resolved directory must stay inside the extension's
/// working directory (spec: `invalid_params` when cwd is outside the worktree).
fn resolve_exec_cwd(
    cwd: &str,
    working_directory: Option<&Path>,
) -> Result<PathBuf, ExtensionError> {
    if cwd.is_empty() {
        return Err(ExtensionError::invalid_params("cwd must not be empty"));
    }
    if !Path::new(cwd).is_absolute() {
        return resolve_path(cwd, working_directory);
    }
    let wd = working_directory
        .ok_or_else(|| ExtensionError::invalid_params("no working directory set"))?;
    // The session working directory is canonicalized (Windows: `\\?\C:\...`
    // verbatim form via fs::canonicalize), while the client cwd is a plain
    // absolute path. Strip verbatim prefixes on both sides so the containment
    // check compares equivalent forms.
    let normalized = normalize_path(&strip_verbatim_prefix(Path::new(cwd)));
    let wd_norm = normalize_path(&strip_verbatim_prefix(wd));
    if !normalized.starts_with(&wd_norm) {
        return Err(ExtensionError::invalid_params(
            "cwd outside working directory",
        ));
    }
    Ok(normalized)
}

/// Strip the Windows `\\?\` (or `\\?\UNC\`) verbatim prefix so paths from
/// `fs::canonicalize` compare equal to their plain forms.
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn resolve_path_for_rename(
    path: &str,
    working_directory: Option<&Path>,
) -> Result<PathBuf, ExtensionError> {
    validate_path_string(path)?;
    let wd = working_directory
        .ok_or_else(|| ExtensionError::invalid_params("no working directory set"))?;
    let normalized = normalize_path(&wd.join(path));
    let wd_norm = normalize_path(wd);
    if !normalized.starts_with(&wd_norm) {
        return Err(ExtensionError::forbidden("path outside working directory"));
    }
    Ok(normalized)
}

fn system_time_to_rfc3339(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    chrono::DateTime::from_timestamp(secs as i64, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

fn file_metadata_to_entry(
    entry: &std::fs::DirEntry,
    relative_to: &Path,
    worktree_root: Option<&Path>,
) -> Result<FileEntry, ExtensionError> {
    let metadata = entry
        .metadata()
        .map_err(|e| ExtensionError::invalid_params(format!("metadata read failed: {e}")))?;

    let name = entry.file_name().to_string_lossy().to_string();
    let full_path = entry.path();
    let rel_path = full_path
        .strip_prefix(relative_to)
        .unwrap_or(&full_path)
        .to_string_lossy()
        .replace('\\', "/");

    let symlink_target = if metadata.file_type().is_symlink() {
        let raw = std::fs::read_link(&full_path).ok();
        raw.map(|target| {
            let canonical_target = target.canonicalize().unwrap_or_else(|_| target.clone());
            if let Some(base) = worktree_root {
                let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
                if canonical_target.starts_with(&canonical_base) {
                    target.to_string_lossy().to_string()
                } else {
                    "<outside worktree>".to_string()
                }
            } else {
                target.to_string_lossy().to_string()
            }
        })
    } else {
        None
    };

    let modified = metadata
        .modified()
        .map(system_time_to_rfc3339)
        .unwrap_or_default();
    let created = metadata.created().ok().map(system_time_to_rfc3339);

    let permissions = format_permissions(&metadata);

    Ok(FileEntry {
        name,
        path: rel_path,
        is_directory: metadata.is_dir(),
        size: metadata.len(),
        modified,
        created,
        permissions: Some(permissions),
        is_symlink: metadata.file_type().is_symlink(),
        symlink_target,
        is_hidden: entry.file_name().to_string_lossy().starts_with('.'),
    })
}

#[cfg(unix)]
fn format_permissions(metadata: &std::fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mode = metadata.permissions().mode();
    let mut s = String::with_capacity(10);
    s.push(if metadata.is_dir() { 'd' } else { '-' });
    let perms = [
        ((mode >> 6) & 0o7, "rwx"),
        ((mode >> 3) & 0o7, "rwx"),
        (mode & 0o7, "rwx"),
    ];
    for (bits, labels) in perms {
        let chars: Vec<char> = labels.chars().collect();
        for (i, c) in chars.iter().enumerate() {
            let bit = 0o4 >> i;
            s.push(if bits & bit != 0 { *c } else { '-' });
        }
    }
    s
}

#[cfg(not(unix))]
fn format_permissions(metadata: &std::fs::Metadata) -> String {
    let readonly = metadata.permissions().readonly();
    if metadata.is_dir() {
        if readonly {
            "rwxr-xr-x".to_string()
        } else {
            "rwxrwxrwx".to_string()
        }
    } else if readonly {
        "rw-r--r--".to_string()
    } else {
        "rw-rw-rw-".to_string()
    }
}

fn infer_mime_type(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext.to_lowercase().as_str() {
        "rs" => "text/x-rust".into(),
        "js" => "text/javascript".into(),
        "ts" => "text/typescript".into(),
        "json" => "application/json".into(),
        "html" => "text/html".into(),
        "css" => "text/css".into(),
        "md" => "text/markdown".into(),
        "txt" => "text/plain".into(),
        "xml" => "text/xml".into(),
        "yaml" | "yml" => "text/yaml".into(),
        "png" => "image/png".into(),
        "jpg" | "jpeg" => "image/jpeg".into(),
        "gif" => "image/gif".into(),
        "svg" => "image/svg+xml".into(),
        "ico" => "image/x-icon".into(),
        "webp" => "image/webp".into(),
        "pdf" => "application/pdf".into(),
        "zip" => "application/zip".into(),
        "gz" | "tgz" => "application/gzip".into(),
        "tar" => "application/x-tar".into(),
        "wav" => "audio/wav".into(),
        "mp3" => "audio/mpeg".into(),
        "mp4" => "video/mp4".into(),
        "webm" => "video/webm".into(),
        "toml" => "text/x-toml".into(),
        "lua" => "text/x-lua".into(),
        "py" => "text/x-python".into(),
        "go" => "text/x-go".into(),
        "java" => "text/x-java".into(),
        "c" | "h" => "text/x-c".into(),
        "cpp" | "hpp" | "cc" => "text/x-c++".into(),
        _ => "application/octet-stream".into(),
    }
}

fn is_executable_binary(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    matches!(
        ext.to_lowercase().as_str(),
        "exe" | "dll" | "so" | "dylib" | "bin" | "sh" | "bat" | "cmd" | "ps1" | "com"
    )
}

fn simple_base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        result.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub size: u64,
    pub modified: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<String>,
    pub is_symlink: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symlink_target: Option<String>,
    pub is_hidden: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileListParams {
    path: Option<String>,
    recursive: Option<bool>,
    show_hidden: Option<bool>,
    cursor: Option<String>,
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileSearchParams {
    query: String,
    path: Option<String>,
    pattern: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct FileStatParams {
    path: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FileStatResult {
    name: String,
    path: String,
    is_directory: bool,
    size: u64,
    modified: String,
    created: Option<String>,
    permissions: Option<String>,
    is_symlink: bool,
    symlink_target: Option<String>,
    is_hidden: bool,
    mime_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateDirectoryParams {
    path: String,
    create_parent: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadBinaryParams {
    path: String,
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadTextParams {
    path: String,
    optional: Option<bool>,
    // Kept so an optional read can distinguish a missing anchor directory
    // from an ordinary unbound workspace request. ExtensionContext remains
    // the authoritative resolved/canonicalized directory.
    directory: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadTextResult {
    path: String,
    content: String,
    encoding: &'static str,
    size: u64,
    found: bool,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadBinaryResult {
    path: String,
    data_url: String,
    mime_type: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteFileParams {
    path: String,
    content: String,
    create_parent: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct DeleteFileParams {
    path: String,
    recursive: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameFileParams {
    old_path: String,
    new_path: String,
}

#[derive(Debug, Deserialize)]
struct RevealPathParams {
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecCommandsParams {
    cwd: String,
    commands: Vec<String>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecResult {
    command: String,
    exit_code: i32,
    stdout: String,
    stderr: String,
    duration_ms: u64,
}

#[derive(Debug, Deserialize)]
struct DownloadFileParams {
    path: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeType {
    Created,
    Deleted,
    Modified,
    Renamed,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileChangedNotification {
    pub change: FileChangeType,
    pub path: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeEntry>,
}

fn build_changed_notification(change: FileChangeType, path: &str) -> Value {
    let notif = FileChangedNotification {
        change,
        path: path.to_string(),
    };
    serde_json::to_value(notif).unwrap_or(Value::Null)
}

fn decode_cursor_offset(cursor: Option<&str>) -> Result<usize, ExtensionError> {
    let Some(cursor) = cursor else { return Ok(0) };
    let bytes = (|| {
        if cursor.len() % 2 != 0 {
            return Err(());
        }
        (0..cursor.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&cursor[i..i + 2], 16).map_err(|_| ()))
            .collect::<Result<Vec<u8>, ()>>()
    })()
    .map_err(|_| ExtensionError::invalid_params("invalid cursor"))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| ExtensionError::invalid_params("invalid cursor"))?;
    Ok(value
        .get("offset")
        .and_then(|o| o.as_u64())
        .map(|n| n as usize)
        .unwrap_or(0))
}

fn match_glob(pattern: &str, name: &str) -> bool {
    let pat = pattern.trim_start_matches("./");
    if pat == "*" || pat.is_empty() {
        return true;
    }
    let lower_name = name.to_lowercase();
    let lower_pat = pat.to_lowercase();
    if let Some(dot_pos) = lower_pat.rfind('.') {
        let pat_ext = &lower_pat[dot_pos..];
        lower_name.ends_with(pat_ext)
    } else {
        lower_name.contains(&lower_pat)
    }
}

fn collect_entries(
    dir: &Path,
    base: &Path,
    recursive: bool,
    show_hidden: bool,
    entries: &mut Vec<FileEntry>,
) -> Result<(), ExtensionError> {
    let read = std::fs::read_dir(dir).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ExtensionError::not_found(format!(
                "directory does not exist: {}",
                dir.to_string_lossy()
            ))
        } else {
            ExtensionError::invalid_params(format!("failed to read directory: {e}"))
        }
    })?;

    for entry in read {
        let entry = entry.map_err(|e| {
            ExtensionError::invalid_params(format!("failed to read directory entry: {e}"))
        })?;

        let is_hidden = entry.file_name().to_string_lossy().starts_with('.');

        if !show_hidden && is_hidden {
            continue;
        }

        let file_entry = file_metadata_to_entry(&entry, base, None)?;

        if recursive && file_entry.is_directory {
            let sub_dir = entry.path();
            let sub_entry = file_entry.clone();
            entries.push(sub_entry);
            collect_entries(&sub_dir, base, true, show_hidden, entries)?;
        } else {
            entries.push(file_entry);
        }
    }

    Ok(())
}

#[async_trait]
impl ExtensionHandler for FilesHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        if FORBIDDEN_METHODS.contains(&method) {
            return Err(ExtensionError::method_not_found());
        }

        match method {
            "list" => self.handle_list(params, ctx).await,
            "search" => self.handle_search(params, ctx).await,
            "stat" => self.handle_stat(params, ctx).await,
            "create_directory" => self.handle_create_directory(params, ctx).await,
            "read_text_file" => self.handle_read_text_file(params, ctx).await,
            "read_file_binary" => self.handle_read_file_binary(params, ctx).await,
            "write_file" => self.handle_write_file(params, ctx).await,
            "delete" => self.handle_delete(params, ctx).await,
            "rename" => self.handle_rename(params, ctx).await,
            "reveal_path" => self.handle_reveal_path(params, ctx).await,
            "exec_commands" => self.handle_exec_commands(params, ctx).await,
            "download_file" => self.handle_download_file(params, ctx).await,
            "home" => self.handle_home().await,
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({
            "list": true,
            "search": true,
            "stat": true,
            "create_directory": true,
            "read_text_file": true,
            "read_file_binary": true,
            "write_file": true,
            "delete": true,
            "rename": true,
            "reveal_path": true,
            "exec_commands": true,
            "download_file": true,
            "home": true
        })
    }
}

impl FilesHandler {
    async fn handle_read_text_file(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let p: ReadTextParams = serde_json::from_value(params)
            .map_err(|e| ExtensionError::invalid_params(format!("{e}")))?;
        let optional = p.optional.unwrap_or(false);
        if ctx.working_directory.is_none() && optional {
            // Optional project config reads may point at a config directory
            // that has not been created yet. No filesystem access is needed
            // to report that expected absence, and it must not become a
            // normal-startup JSON-RPC error frame.
            if p.directory
                .as_deref()
                .is_some_and(|directory| !Path::new(directory).is_dir())
            {
                return serde_json::to_value(ReadTextResult {
                    path: p.path,
                    content: String::new(),
                    encoding: "utf-8",
                    size: 0,
                    found: false,
                })
                .map_err(|e| ExtensionError::invalid_params(format!("serialization failed: {e}")));
            }
        }
        let resolved = resolve_path(&p.path, ctx.working_directory.as_deref())?;
        let metadata = match std::fs::metadata(&resolved) {
            Ok(metadata) => metadata,
            Err(error) if optional && error.kind() == std::io::ErrorKind::NotFound => {
                return serde_json::to_value(ReadTextResult {
                    path: p.path,
                    content: String::new(),
                    encoding: "utf-8",
                    size: 0,
                    found: false,
                })
                .map_err(|e| ExtensionError::invalid_params(format!("serialization failed: {e}")));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ExtensionError::not_found(format!(
                    "file not found: {}",
                    p.path
                )));
            }
            Err(error) => {
                return Err(ExtensionError::invalid_params(format!(
                    "stat failed: {error}"
                )));
            }
        };
        if metadata.is_dir() {
            return Err(ExtensionError::invalid_params("path is a directory"));
        }
        if metadata.len() > MAX_TEXT_READ_SIZE {
            return Err(ExtensionError::invalid_params(format!(
                "file exceeds maximum text size of {MAX_TEXT_READ_SIZE} bytes"
            )));
        }
        let content = std::fs::read_to_string(&resolved).map_err(|e| {
            if e.kind() == std::io::ErrorKind::InvalidData {
                ExtensionError::invalid_params("file is not valid UTF-8")
            } else {
                ExtensionError::invalid_params(format!("read failed: {e}"))
            }
        })?;
        serde_json::to_value(ReadTextResult {
            path: p.path,
            content,
            encoding: "utf-8",
            size: metadata.len(),
            found: true,
        })
        .map_err(|e| ExtensionError::invalid_params(format!("serialization failed: {e}")))
    }

    async fn handle_home(&self) -> Result<Value, ExtensionError> {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map_err(|_| ExtensionError {
                code: -32603,
                message: "internal_error".into(),
                data: Some(serde_json::json!("home directory unavailable")),
            })?;
        Ok(serde_json::json!({ "home": home }))
    }

    async fn handle_list(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let p: FileListParams = serde_json::from_value(params)
            .map_err(|e| ExtensionError::invalid_params(format!("{e}")))?;

        let sub_path = p.path.as_deref().unwrap_or(".");
        let working_dir = ctx.working_directory.as_deref();

        let resolved = if sub_path == "." {
            working_dir
                .map(|w| w.to_path_buf())
                .ok_or_else(|| ExtensionError::invalid_params("no working directory set"))?
        } else {
            resolve_path(sub_path, working_dir)?
        };

        if !resolved.is_dir() {
            return Err(ExtensionError::not_found(format!(
                "path is not a directory: {sub_path}"
            )));
        }

        let base = working_dir
            .map(|w| w.to_path_buf())
            .unwrap_or_else(|| resolved.clone());

        let mut entries = Vec::new();
        collect_entries(
            &resolved,
            &base,
            p.recursive.unwrap_or(false),
            p.show_hidden.unwrap_or(false),
            &mut entries,
        )?;

        entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });

        let limit = p.limit.unwrap_or(DEFAULT_LIMIT as u32) as usize;
        let limit = limit.min(MAX_LIMIT);
        let offset = decode_cursor_offset(p.cursor.as_deref())?;

        let page = PaginatedResult::from_slice(entries, offset, limit);
        Ok(page.to_json())
    }

    async fn handle_search(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let p: FileSearchParams = serde_json::from_value(params)
            .map_err(|e| ExtensionError::invalid_params(format!("{e}")))?;

        if p.query.is_empty() {
            return Err(ExtensionError::invalid_params("query must not be empty"));
        }

        let working_dir = ctx.working_directory.as_deref();
        let search_root = if let Some(search_path) = &p.path {
            resolve_path(search_path, working_dir)?
        } else {
            working_dir
                .map(|w| w.to_path_buf())
                .ok_or_else(|| ExtensionError::invalid_params("no working directory set"))?
        };

        if !search_root.is_dir() {
            return Err(ExtensionError::not_found("search path is not a directory"));
        }

        let base = working_dir
            .map(|w| w.to_path_buf())
            .unwrap_or_else(|| search_root.clone());

        let query_lower = p.query.to_lowercase();
        let mut entries = Vec::new();

        {
            let mut builder = WalkBuilder::new(&search_root);
            builder
                .hidden(false)
                .parents(true)
                .git_ignore(true)
                .git_global(true)
                .git_exclude(true)
                .follow_links(false)
                .max_depth(None);

            let walker = builder.build();

            for result in walker {
                let entry = match result {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                if entry.depth() == 0 {
                    continue;
                }

                let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);

                if is_dir && p.pattern.is_some() {
                    continue;
                }

                let name = entry.file_name().to_string_lossy().to_string();
                let is_hidden = name.starts_with('.');

                let name_match = name.to_lowercase().contains(&query_lower);
                let pattern_match = p
                    .pattern
                    .as_ref()
                    .map(|pat| match_glob(pat, &name))
                    .unwrap_or(true);

                if is_hidden && !name_match {
                    continue;
                }

                if name_match && pattern_match {
                    let fe = FileEntry {
                        name: name.clone(),
                        path: entry
                            .path()
                            .strip_prefix(&base)
                            .unwrap_or(entry.path())
                            .to_string_lossy()
                            .replace('\\', "/"),
                        is_directory: is_dir,
                        size: entry.metadata().map(|m| m.len()).unwrap_or(0),
                        modified: entry
                            .metadata()
                            .map(|m| m.modified().ok())
                            .ok()
                            .flatten()
                            .map(system_time_to_rfc3339)
                            .unwrap_or_default(),
                        created: entry
                            .metadata()
                            .map(|m| m.created().ok())
                            .ok()
                            .flatten()
                            .map(system_time_to_rfc3339),
                        permissions: entry.metadata().map(|m| format_permissions(&m)).ok(),
                        is_symlink: entry.file_type().map(|ft| ft.is_symlink()).unwrap_or(false),
                        symlink_target: None,
                        is_hidden,
                    };
                    entries.push(fe);
                }
            }
        }

        let limit = p.limit.unwrap_or(DEFAULT_LIMIT as u32) as usize;
        let limit = limit.min(MAX_LIMIT);
        let offset = decode_cursor_offset(p.cursor.as_deref())?;

        let page = PaginatedResult::from_slice(entries, offset, limit);
        Ok(page.to_json())
    }

    async fn handle_stat(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let p: FileStatParams = serde_json::from_value(params)
            .map_err(|e| ExtensionError::invalid_params(format!("{e}")))?;

        let working_dir = ctx.working_directory.as_deref();
        let resolved = resolve_path(&p.path, working_dir)?;
        let base = working_dir.map(|w| w.to_path_buf());

        let metadata = std::fs::symlink_metadata(&resolved).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ExtensionError::not_found(format!("path does not exist: {}", p.path))
            } else {
                ExtensionError::invalid_params(format!("failed to stat: {e}"))
            }
        })?;

        let name = resolved
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let rel_path = if let Some(base) = &base {
            resolved
                .strip_prefix(base)
                .unwrap_or(&resolved)
                .to_string_lossy()
                .replace('\\', "/")
        } else {
            p.path.clone()
        };

        let symlink_target = if metadata.file_type().is_symlink() {
            let raw = std::fs::read_link(&resolved).ok();
            raw.map(|target| {
                let canonical_target = target.canonicalize().unwrap_or(target.clone());
                if let Some(base) = &base {
                    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.clone());
                    if canonical_target.starts_with(&canonical_base) {
                        target.to_string_lossy().to_string()
                    } else {
                        "<outside worktree>".to_string()
                    }
                } else {
                    target.to_string_lossy().to_string()
                }
            })
        } else {
            None
        };

        let result = FileStatResult {
            name,
            path: rel_path,
            is_directory: metadata.is_dir(),
            size: metadata.len(),
            modified: metadata
                .modified()
                .map(system_time_to_rfc3339)
                .unwrap_or_default(),
            created: metadata.created().ok().map(system_time_to_rfc3339),
            permissions: Some(format_permissions(&metadata)),
            is_symlink: metadata.file_type().is_symlink(),
            symlink_target,
            is_hidden: resolved
                .file_name()
                .map(|n| n.to_string_lossy().starts_with('.'))
                .unwrap_or(false),
            mime_type: if metadata.is_dir() {
                String::new()
            } else {
                infer_mime_type(&p.path)
            },
        };

        serde_json::to_value(result)
            .map_err(|e| ExtensionError::invalid_params(format!("serialization failed: {e}")))
    }

    async fn handle_create_directory(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let p: CreateDirectoryParams = serde_json::from_value(params)
            .map_err(|e| ExtensionError::invalid_params(format!("{e}")))?;

        auth::check_server_policy(ctx, "files", "create_directory")?;

        let working_dir = ctx.working_directory.as_deref();
        let resolved = resolve_path(&p.path, working_dir)?;
        let base = working_dir.map(|w| w.to_path_buf());

        if resolved.exists() {
            if !resolved.is_dir() {
                return Err(ExtensionError::conflict(
                    "path exists and is not a directory",
                ));
            }
            if p.create_parent.unwrap_or(false) {
                let rel = if let Some(base) = &base {
                    resolved
                        .strip_prefix(base)
                        .unwrap_or(&resolved)
                        .to_string_lossy()
                        .replace('\\', "/")
                } else {
                    p.path.clone()
                };
                return Ok(serde_json::json!({ "path": rel, "created": false }));
            }
            return Err(ExtensionError::conflict("directory already exists"));
        }

        if p.create_parent.unwrap_or(false) {
            std::fs::create_dir_all(&resolved)
                .map_err(|e| ExtensionError::invalid_params(format!("mkdir failed: {e}")))?;
        } else {
            std::fs::create_dir(&resolved)
                .map_err(|e| ExtensionError::invalid_params(format!("mkdir failed: {e}")))?;
        }

        let rel = if let Some(base) = &base {
            resolved
                .strip_prefix(base)
                .unwrap_or(&resolved)
                .to_string_lossy()
                .replace('\\', "/")
        } else {
            p.path.clone()
        };

        Ok(serde_json::json!({
            "path": rel,
            "created": true,
            "notification": build_changed_notification(FileChangeType::Created, &rel)
        }))
    }

    async fn handle_read_file_binary(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let p: ReadBinaryParams = serde_json::from_value(params)
            .map_err(|e| ExtensionError::invalid_params(format!("{e}")))?;

        let working_dir = ctx.working_directory.as_deref();
        let resolved = resolve_path(&p.path, working_dir)?;

        let metadata = std::fs::metadata(&resolved).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ExtensionError::not_found(format!("file not found: {}", p.path))
            } else {
                ExtensionError::invalid_params(format!("stat failed: {e}"))
            }
        })?;

        if metadata.len() > MAX_BINARY_READ_SIZE {
            return Err(ExtensionError::invalid_params(format!(
                "file exceeds maximum size of {} bytes",
                MAX_BINARY_READ_SIZE
            )));
        }

        let detected_mime = infer_mime_type(&p.path);

        if let Some(expected) = &p.mime_type {
            if expected != &detected_mime {
                return Err(ExtensionError::invalid_params(format!(
                    "mimeType mismatch: expected {expected}, got {detected_mime}"
                )));
            }
        }

        if is_executable_binary(&p.path) {
            return Err(ExtensionError::forbidden(
                "reading binary executable files is not allowed",
            ));
        }

        let data = std::fs::read(&resolved)
            .map_err(|e| ExtensionError::invalid_params(format!("read failed: {e}")))?;

        let b64 = simple_base64_encode(&data);
        let result = ReadBinaryResult {
            path: p.path,
            data_url: format!("data:{detected_mime};base64,{b64}"),
            mime_type: detected_mime,
            size: data.len() as u64,
        };

        serde_json::to_value(result)
            .map_err(|e| ExtensionError::invalid_params(format!("serialization failed: {e}")))
    }

    async fn handle_write_file(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let p: WriteFileParams = serde_json::from_value(params)
            .map_err(|e| ExtensionError::invalid_params(format!("{e}")))?;

        auth::check_server_policy(ctx, "files", "write_file")?;

        let working_dir = ctx.working_directory.as_deref();
        let resolved = resolve_path(&p.path, working_dir)?;

        if p.create_parent.unwrap_or(false) {
            if let Some(parent) = resolved.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ExtensionError::invalid_params(format!("create parent failed: {e}"))
                })?;
            }
        } else if let Some(parent) = resolved.parent() {
            if !parent.exists() {
                return Err(ExtensionError::not_found("parent directory does not exist"));
            }
        }

        std::fs::write(&resolved, p.content.as_bytes())
            .map_err(|e| ExtensionError::invalid_params(format!("write failed: {e}")))?;

        let size = p.content.len() as u64;

        Ok(serde_json::json!({
            "path": p.path,
            "written": true,
            "size": size,
            "notification": build_changed_notification(FileChangeType::Modified, &p.path)
        }))
    }

    async fn handle_delete(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let p: DeleteFileParams = serde_json::from_value(params)
            .map_err(|e| ExtensionError::invalid_params(format!("{e}")))?;

        auth::check_server_policy(ctx, "files", "delete")?;

        let working_dir = ctx.working_directory.as_deref();
        let resolved = resolve_path(&p.path, working_dir)?;

        if let Some(base) = working_dir {
            let root_norm = normalize_path(base);
            if resolved == root_norm {
                return Err(ExtensionError::forbidden("cannot delete worktree root"));
            }
        }

        if !resolved.exists() {
            return Err(ExtensionError::not_found(format!(
                "path not found: {}",
                p.path
            )));
        }

        if resolved.is_dir() && !p.recursive.unwrap_or(false) {
            let is_empty = std::fs::read_dir(&resolved)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false);
            if !is_empty {
                return Err(ExtensionError::invalid_params(
                    "cannot delete non-empty directory without recursive=true",
                ));
            }
        }

        if resolved.is_dir() && p.recursive.unwrap_or(false) {
            std::fs::remove_dir_all(&resolved)
                .map_err(|e| ExtensionError::invalid_params(format!("delete failed: {e}")))?;
        } else if resolved.is_dir() {
            std::fs::remove_dir(&resolved)
                .map_err(|e| ExtensionError::invalid_params(format!("delete failed: {e}")))?;
        } else {
            std::fs::remove_file(&resolved)
                .map_err(|e| ExtensionError::invalid_params(format!("delete failed: {e}")))?;
        }

        Ok(serde_json::json!({
            "path": p.path,
            "deleted": true,
            "notification": build_changed_notification(FileChangeType::Deleted, &p.path)
        }))
    }

    async fn handle_rename(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let p: RenameFileParams = serde_json::from_value(params)
            .map_err(|e| ExtensionError::invalid_params(format!("{e}")))?;

        auth::check_server_policy(ctx, "files", "rename")?;

        let working_dir = ctx.working_directory.as_deref();
        let old_resolved = resolve_path_for_rename(&p.old_path, working_dir)?;
        let new_resolved = resolve_path_for_rename(&p.new_path, working_dir)?;

        if !old_resolved.exists() {
            return Err(ExtensionError::not_found(format!(
                "source path not found: {}",
                p.old_path
            )));
        }

        if new_resolved.exists() {
            let same_file = old_resolved
                .canonicalize()
                .ok()
                .and_then(|old| new_resolved.canonicalize().ok().map(|new| old == new))
                .unwrap_or(false);
            if !same_file {
                return Err(ExtensionError::conflict("destination already exists"));
            }
        }

        std::fs::rename(&old_resolved, &new_resolved).map_err(|e| {
            if e.kind() == std::io::ErrorKind::CrossesDevices {
                ExtensionError {
                    code: -32603,
                    message: "internal_error".into(),
                    data: Some(Value::String(format!(
                        "cross-device rename not supported: {e}"
                    ))),
                }
            } else {
                ExtensionError::invalid_params(format!("rename failed: {e}"))
            }
        })?;

        Ok(serde_json::json!({
            "oldPath": p.old_path,
            "newPath": p.new_path,
            "renamed": true,
            "notification": build_changed_notification(FileChangeType::Renamed, &p.new_path)
        }))
    }

    async fn handle_reveal_path(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let p: RevealPathParams = serde_json::from_value(params)
            .map_err(|e| ExtensionError::invalid_params(format!("{e}")))?;

        let working_dir = ctx.working_directory.as_deref();
        let resolved = resolve_path(&p.path, working_dir)?;

        if !resolved.exists() {
            return Err(ExtensionError::not_found(format!(
                "path not found: {}",
                p.path
            )));
        }

        #[cfg(target_os = "windows")]
        let reveal_result = {
            let arg = if resolved.is_dir() {
                resolved.as_os_str().to_string_lossy().into_owned()
            } else {
                match resolved.parent() {
                    Some(parent) => parent.as_os_str().to_string_lossy().into_owned(),
                    None => resolved.as_os_str().to_string_lossy().into_owned(),
                }
            };
            tokio::process::Command::new("explorer.exe")
                .arg(&arg)
                .spawn()
                .map(|_| ())
        };
        #[cfg(not(target_os = "windows"))]
        let reveal_result = {
            use tokio::io::AsyncWriteExt;
            let child = if cfg!(target_os = "macos") {
                tokio::process::Command::new("open").arg(&resolved).spawn()
            } else {
                tokio::process::Command::new("xdg-open")
                    .arg(&resolved)
                    .spawn()
            };
            let mut child = match child {
                Ok(child) => child,
                Err(error) => {
                    return Err(ExtensionError {
                        code: -32603,
                        message: "internal_error".into(),
                        data: Some(Value::String(format!(
                            "failed to open system file manager: {error}"
                        ))),
                    });
                }
            };
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(b"").await;
            }
            child.wait().await.map(|_| ())
        };

        match reveal_result {
            Ok(()) => Ok(serde_json::json!({ "path": p.path, "revealed": true })),
            Err(e) => Err(ExtensionError {
                code: -32603,
                message: "internal_error".into(),
                data: Some(Value::String(format!(
                    "failed to open system file manager: {e}"
                ))),
            }),
        }
    }

    async fn handle_exec_commands(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let p: ExecCommandsParams = serde_json::from_value(params)
            .map_err(|e| ExtensionError::invalid_params(format!("{e}")))?;

        auth::check_server_policy(ctx, "files", "exec")?;

        if p.commands.is_empty() {
            return Ok(serde_json::json!({ "results": [] }));
        }

        let working_dir = ctx.working_directory.as_deref();
        let cwd = resolve_exec_cwd(&p.cwd, working_dir)?;

        if !cwd.is_dir() {
            return Err(ExtensionError::invalid_params("cwd is not a directory"));
        }

        let timeout_ms = p.timeout_ms.unwrap_or(DEFAULT_EXEC_TIMEOUT_MS);

        let mut results = Vec::new();

        for cmd in &p.commands {
            let start = std::time::Instant::now();

            let (shell, shell_flag) = if cfg!(target_os = "windows") {
                ("cmd", "/C")
            } else {
                ("sh", "-c")
            };

            let mut child = tokio::process::Command::new(shell);
            child
                .arg(shell_flag)
                .arg(cmd)
                .current_dir(&cwd)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            #[cfg(windows)]
            {
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                child.creation_flags(CREATE_NO_WINDOW);
            }
            let mut child = child
                .spawn()
                .map_err(|e| ExtensionError::invalid_params(format!("spawn failed: {e}")))?;

            let mut child_stdout = child.stdout.take();
            let mut child_stderr = child.stderr.take();

            let status = tokio::select! {
                status = child.wait() => status,
                _ = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return Err(ExtensionError {
                        code: -32004,
                        message: "timeout".into(),
                        data: Some(Value::String(format!(
                            "command exceeded timeout of {timeout_ms}ms"
                        ))),
                    });
                }
            };

            let mut stdout_buf = Vec::new();
            if let Some(ref mut so) = child_stdout {
                use tokio::io::AsyncReadExt;
                let _ = so.read_to_end(&mut stdout_buf).await;
            }
            let mut stderr_buf = Vec::new();
            if let Some(ref mut se) = child_stderr {
                use tokio::io::AsyncReadExt;
                let _ = se.read_to_end(&mut stderr_buf).await;
            }

            let status = status.map_err(|e| {
                ExtensionError::invalid_params(format!("command execution failed: {e}"))
            })?;

            let duration_ms = start.elapsed().as_millis() as u64;
            let exit_code = status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&stdout_buf).to_string();
            let stderr = String::from_utf8_lossy(&stderr_buf).to_string();

            let exec_result = ExecResult {
                command: cmd.clone(),
                exit_code,
                stdout,
                stderr,
                duration_ms,
            };

            let exit_was_zero = exit_code == 0;
            results.push(exec_result);

            if !exit_was_zero {
                break;
            }
        }

        serde_json::to_value(serde_json::json!({ "results": results }))
            .map_err(|e| ExtensionError::invalid_params(format!("serialization failed: {e}")))
    }

    async fn handle_download_file(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let p: DownloadFileParams = serde_json::from_value(params)
            .map_err(|e| ExtensionError::invalid_params(format!("{e}")))?;

        let working_dir = ctx.working_directory.as_deref();
        let resolved = resolve_path(&p.path, working_dir)?;

        let metadata = std::fs::metadata(&resolved).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ExtensionError::not_found(format!("file not found: {}", p.path))
            } else {
                ExtensionError::invalid_params(format!("stat failed: {e}"))
            }
        })?;

        if metadata.len() > MAX_DOWNLOAD_SIZE {
            return Err(ExtensionError::invalid_params(
                "file too large for download",
            ));
        }

        let mime = infer_mime_type(&p.path);
        let download_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now();
        let expires = now + chrono::Duration::minutes(30);

        Ok(serde_json::json!({
            "path": p.path,
            "downloadUrl": format!("blob:anureo/download/{download_id}"),
            "mimeType": mime,
            "size": metadata.len(),
            "expiresAt": expires.to_rfc3339()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_capabilities::ClientCapabilitiesInfo;
    use crate::extensions::{ExtensionContext, ExtensionHandler};
    use std::fs;
    use tempfile::TempDir;

    fn make_ctx(dir: &std::path::Path) -> ExtensionContext {
        ExtensionContext {
            session_id: Some("test-session".into()),
            principal: "test-user".into(),
            connection_id: "test-conn".into(),
            working_directory: Some(dir.to_path_buf()),
            client_capabilities: ClientCapabilitiesInfo::default(),
        }
    }

    fn make_ctx_no_dir() -> ExtensionContext {
        ExtensionContext {
            session_id: Some("test-session".into()),
            principal: "test-user".into(),
            connection_id: "test-conn".into(),
            working_directory: None,
            client_capabilities: ClientCapabilitiesInfo::default(),
        }
    }

    fn setup_workdir() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("src/lib.rs"), "pub mod foo;").unwrap();
        fs::write(root.join("README.md"), "# Project").unwrap();
        fs::write(root.join(".hidden"), "secret").unwrap();
        fs::write(root.join("src/test.rs"), "mod tests;").unwrap();
        tmp
    }

    // ── validate_path_string ─────────────────────────────

    #[test]
    fn test_validate_path_string_empty() {
        assert!(validate_path_string("").is_err());
    }

    #[test]
    fn test_validate_path_string_absolute_unix() {
        assert!(validate_path_string("/etc/passwd").is_err());
    }

    #[test]
    fn test_validate_path_string_absolute_windows() {
        assert!(validate_path_string("\\windows\\system32").is_err());
        assert!(validate_path_string("C:stuff").is_err());
    }

    #[test]
    fn test_validate_path_string_traversal() {
        assert!(validate_path_string("../secret").is_err());
        assert!(validate_path_string("src/../../etc").is_err());
        assert!(validate_path_string("src/../..").is_err());
    }

    #[test]
    fn test_validate_path_string_valid() {
        assert!(validate_path_string("src/main.rs").is_ok());
        assert!(validate_path_string("docs").is_ok());
        assert!(validate_path_string(".").is_ok());
    }

    // ── forbidden methods ────────────────────────────────

    #[tokio::test]
    async fn test_forbidden_method_read() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("read", serde_json::json!({"path": "src"}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32601);
    }

    #[tokio::test]
    async fn test_read_text_file() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "read_text_file",
                serde_json::json!({"path": "src/main.rs"}),
                &ctx,
            )
            .await;
        let result = result.expect("text read should succeed");
        assert_eq!(result["content"], "fn main() {}");
        assert_eq!(result["encoding"], "utf-8");
        assert_eq!(result["size"], 12);
        assert_eq!(result["found"], true);
    }

    #[tokio::test]
    async fn test_optional_read_text_file_returns_found_false_for_missing_file() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let result = FilesHandler::new()
            .handle(
                "read_text_file",
                serde_json::json!({"path": "missing.json", "optional": true}),
                &ctx,
            )
            .await
            .expect("optional missing file should be a successful absence");
        assert_eq!(result["found"], false);
        assert_eq!(result["content"], "");
    }

    #[tokio::test]
    async fn test_optional_read_text_file_returns_found_false_for_missing_anchor() {
        let ctx = make_ctx_no_dir();
        let missing = tempfile::tempdir().unwrap().path().join("not-created");
        let result = FilesHandler::new()
            .handle(
                "read_text_file",
                serde_json::json!({
                    "path": "project.json",
                    "directory": missing.to_string_lossy(),
                    "optional": true
                }),
                &ctx,
            )
            .await
            .expect("optional missing anchor should be a successful absence");
        assert_eq!(result["found"], false);
    }

    // ── unknown method ───────────────────────────────────

    #[tokio::test]
    async fn test_unknown_method() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("nonexistent", serde_json::json!({}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32601);
    }

    // ── capabilities ─────────────────────────────────────

    #[test]
    fn test_capabilities_shape() {
        let handler = FilesHandler::new();
        let caps = handler.capabilities();
        assert_eq!(caps["list"], true);
        assert_eq!(caps["search"], true);
        assert_eq!(caps["stat"], true);
        assert_eq!(caps["write_file"], true);
        assert_eq!(caps["delete"], true);
        assert_eq!(caps["rename"], true);
    }

    // ── list ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 0);
        assert_eq!(result["hasMore"], false);
        assert!(result["nextCursor"].is_null());
    }

    #[tokio::test]
    async fn test_list_returns_entries() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();
        let items = result["items"].as_array().unwrap();
        assert!(items.len() >= 3);
        let names: Vec<&str> = items.iter().map(|i| i["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"src"));
        assert!(names.contains(&"docs"));
        assert!(names.contains(&"README.md"));
    }

    #[tokio::test]
    async fn test_list_hides_hidden_by_default() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();
        let items = result["items"].as_array().unwrap();
        let names: Vec<&str> = items.iter().map(|i| i["name"].as_str().unwrap()).collect();
        assert!(!names.contains(&".hidden"));
    }

    #[tokio::test]
    async fn test_list_shows_hidden_when_requested() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("list", serde_json::json!({"showHidden": true}), &ctx)
            .await
            .unwrap();
        let items = result["items"].as_array().unwrap();
        let names: Vec<&str> = items.iter().map(|i| i["name"].as_str().unwrap()).collect();
        assert!(names.contains(&".hidden"));
    }

    #[tokio::test]
    async fn test_list_recursive() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("list", serde_json::json!({"recursive": true}), &ctx)
            .await
            .unwrap();
        let items = result["items"].as_array().unwrap();
        let names: Vec<&str> = items.iter().map(|i| i["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"main.rs"));
        assert!(names.contains(&"lib.rs"));
    }

    #[tokio::test]
    async fn test_list_nonexistent_dir() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("list", serde_json::json!({"path": "noexist"}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32003);
    }

    #[tokio::test]
    async fn test_list_path_traversal_rejected() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("list", serde_json::json!({"path": "../"}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32602);
    }

    #[tokio::test]
    async fn test_list_absolute_path_rejected() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("list", serde_json::json!({"path": "/etc"}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32602);
    }

    #[tokio::test]
    async fn test_list_pagination_limit() {
        let tmp = TempDir::new().unwrap();
        for i in 0..10 {
            fs::write(tmp.path().join(format!("file{i}.txt")), "x").unwrap();
        }
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("list", serde_json::json!({"limit": 3}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 3);
        assert_eq!(result["hasMore"], true);
        assert!(result["nextCursor"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_list_pagination_cursor_round_trip() {
        let tmp = TempDir::new().unwrap();
        for i in 0..10 {
            fs::write(tmp.path().join(format!("file{i}.txt")), "x").unwrap();
        }
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let page1 = handler
            .handle("list", serde_json::json!({"limit": 3}), &ctx)
            .await
            .unwrap();
        assert_eq!(page1["hasMore"], true);
        let cursor = page1["nextCursor"].as_str().unwrap();
        let page2 = handler
            .handle(
                "list",
                serde_json::json!({"limit": 3, "cursor": cursor}),
                &ctx,
            )
            .await
            .unwrap();
        let p1: Vec<&str> = page1["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["name"].as_str().unwrap())
            .collect();
        let p2: Vec<&str> = page2["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["name"].as_str().unwrap())
            .collect();
        assert!(
            p1.iter().all(|n| !p2.contains(n)),
            "page 2 should have different items"
        );
    }

    // ── search ───────────────────────────────────────────

    #[tokio::test]
    async fn test_search_basic() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("search", serde_json::json!({"query": "main"}), &ctx)
            .await
            .unwrap();
        let items = result["items"].as_array().unwrap();
        assert!(!items.is_empty());
        let name = items[0]["name"].as_str().unwrap();
        assert!(name.contains("main"));
    }

    #[tokio::test]
    async fn test_search_empty_query() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("search", serde_json::json!({"query": ""}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32602);
    }

    #[tokio::test]
    async fn test_search_pattern() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "search",
                serde_json::json!({"query": "", "pattern": "*.rs"}),
                &ctx,
            )
            .await;
        assert!(result.is_err(), "empty query should still fail");
    }

    #[tokio::test]
    async fn test_search_pattern_with_query() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "search",
                serde_json::json!({"query": "lib", "pattern": "*.rs"}),
                &ctx,
            )
            .await
            .unwrap();
        let items = result["items"].as_array().unwrap();
        assert!(!items.is_empty());
        let name = items[0]["name"].as_str().unwrap();
        assert!(name.contains("lib"));
    }

    #[tokio::test]
    async fn test_search_no_results() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "search",
                serde_json::json!({"query": "nonexistent_file_xyz"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 0);
        assert_eq!(result["hasMore"], false);
    }

    #[tokio::test]
    async fn test_search_pagination() {
        let tmp = TempDir::new().unwrap();
        for i in 0..5 {
            fs::write(tmp.path().join(format!("match_{i}.txt")), "x").unwrap();
        }
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "search",
                serde_json::json!({"query": "match", "limit": 2}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 2);
        assert_eq!(result["hasMore"], true);
    }

    // ── stat ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_stat_file() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("stat", serde_json::json!({"path": "README.md"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["name"], "README.md");
        assert_eq!(result["isDirectory"], false);
        assert!(result["size"].as_u64().unwrap() > 0);
        assert!(result["mimeType"].as_str().unwrap().contains("markdown"));
    }

    #[tokio::test]
    async fn test_stat_directory() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("stat", serde_json::json!({"path": "src"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["name"], "src");
        assert_eq!(result["isDirectory"], true);
    }

    #[tokio::test]
    async fn test_stat_nonexistent() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("stat", serde_json::json!({"path": "noexist.txt"}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32003);
    }

    #[tokio::test]
    async fn test_stat_traversal_rejected() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("stat", serde_json::json!({"path": "../etc/passwd"}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32602);
    }

    // ── create_directory ─────────────────────────────────

    #[tokio::test]
    async fn test_create_directory() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "create_directory",
                serde_json::json!({"path": "newdir"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result["created"], true);
        assert!(tmp.path().join("newdir").exists());
    }

    #[tokio::test]
    async fn test_create_directory_with_parent() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "create_directory",
                serde_json::json!({"path": "a/b/c", "createParent": true}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result["created"], true);
        assert!(tmp.path().join("a/b/c").exists());
    }

    #[tokio::test]
    async fn test_create_directory_conflict() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        handler
            .handle(
                "create_directory",
                serde_json::json!({"path": "newdir"}),
                &ctx,
            )
            .await
            .unwrap();
        let result = handler
            .handle(
                "create_directory",
                serde_json::json!({"path": "newdir"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32005);
    }

    #[tokio::test]
    async fn test_create_directory_idempotent() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        handler
            .handle(
                "create_directory",
                serde_json::json!({"path": "newdir"}),
                &ctx,
            )
            .await
            .unwrap();
        let result = handler
            .handle(
                "create_directory",
                serde_json::json!({"path": "newdir", "createParent": true}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result["created"], false);
    }

    // ── write_file ───────────────────────────────────────

    #[tokio::test]
    async fn test_write_file() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "write_file",
                serde_json::json!({"path": "hello.txt", "content": "world"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result["written"], true);
        assert_eq!(result["size"], 5);
        assert_eq!(
            fs::read_to_string(tmp.path().join("hello.txt")).unwrap(),
            "world"
        );
    }

    #[tokio::test]
    async fn test_write_file_create_parent() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "write_file",
                serde_json::json!({"path": "newdir/file.txt", "content": "x", "createParent": true}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result["written"], true);
    }

    #[tokio::test]
    async fn test_write_file_missing_parent() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "write_file",
                serde_json::json!({"path": "nodir/file.txt", "content": "x"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32003);
    }

    #[tokio::test]
    async fn test_write_file_traversal_rejected() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "write_file",
                serde_json::json!({"path": "../escape.txt", "content": "x"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32602);
    }

    // ── delete ───────────────────────────────────────────

    #[tokio::test]
    async fn test_delete_file() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("delete", serde_json::json!({"path": "README.md"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["deleted"], true);
        assert!(!tmp.path().join("README.md").exists());
    }

    #[tokio::test]
    async fn test_delete_empty_dir() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        fs::create_dir(tmp.path().join("emptydir")).unwrap();
        let result = handler
            .handle("delete", serde_json::json!({"path": "emptydir"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["deleted"], true);
    }

    #[tokio::test]
    async fn test_delete_nonempty_dir_without_recursive() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("delete", serde_json::json!({"path": "src"}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32602);
    }

    #[tokio::test]
    async fn test_delete_nonempty_dir_with_recursive() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "delete",
                serde_json::json!({"path": "src", "recursive": true}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result["deleted"], true);
        assert!(!tmp.path().join("src").exists());
    }

    #[tokio::test]
    async fn test_delete_nonexistent() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("delete", serde_json::json!({"path": "noexist"}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32003);
    }

    // ── rename ───────────────────────────────────────────

    #[tokio::test]
    async fn test_rename_file() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "rename",
                serde_json::json!({"oldPath": "README.md", "newPath": "readme.md"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result["renamed"], true);
        assert!(tmp.path().join("readme.md").exists());
        #[cfg(unix)]
        assert!(!tmp.path().join("README.md").exists());
        #[cfg(windows)]
        {
            let on_disk = fs::read_dir(tmp.path())
                .unwrap()
                .find_map(|e| {
                    e.ok().filter(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .eq_ignore_ascii_case("readme.md")
                    })
                })
                .expect("readme.md should exist");
            assert_eq!(
                on_disk.file_name().to_string_lossy(),
                "readme.md",
                "on-disk case should be updated"
            );
        }
    }

    #[tokio::test]
    async fn test_rename_conflict() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "rename",
                serde_json::json!({"oldPath": "README.md", "newPath": "src"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32005);
    }

    #[tokio::test]
    async fn test_rename_source_not_found() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "rename",
                serde_json::json!({"oldPath": "noexist.txt", "newPath": "new.txt"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32003);
    }

    // ── reveal_path ──────────────────────────────────────

    #[tokio::test]
    async fn test_reveal_path_existing() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("reveal_path", serde_json::json!({"path": "src"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result["revealed"], true);
    }

    #[tokio::test]
    async fn test_reveal_path_nonexistent() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle("reveal_path", serde_json::json!({"path": "noexist"}), &ctx)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32003);
    }

    // ── download_file ────────────────────────────────────

    #[tokio::test]
    async fn test_download_file() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "download_file",
                serde_json::json!({"path": "README.md"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result["downloadUrl"].as_str().unwrap().contains("blob:"));
        assert!(!result["expiresAt"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_download_file_nonexistent() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "download_file",
                serde_json::json!({"path": "noexist"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32003);
    }

    // ── exec_commands ────────────────────────────────────

    #[tokio::test]
    async fn test_exec_commands_empty() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let result = handler
            .handle(
                "exec_commands",
                serde_json::json!({"cwd": ".", "commands": []}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result["results"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_exec_commands_success() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let cmd = "echo hello";
        let result = handler
            .handle(
                "exec_commands",
                serde_json::json!({"cwd": ".", "commands": [cmd]}),
                &ctx,
            )
            .await
            .unwrap();
        let results = result["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["exitCode"], 0);
        assert!(results[0]["stdout"].as_str().unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn test_exec_commands_stops_on_failure() {
        let tmp = setup_workdir();
        let ctx = make_ctx(tmp.path());
        let handler = FilesHandler::new();
        let cmd_fail = if cfg!(windows) { "exit /b 1" } else { "false" };
        let cmd_ok = "echo ok";
        let result = handler
            .handle(
                "exec_commands",
                serde_json::json!({"cwd": ".", "commands": [cmd_fail, cmd_ok]}),
                &ctx,
            )
            .await
            .unwrap();
        let results = result["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_ne!(results[0]["exitCode"], 0);
    }

    // ── resolve_exec_cwd ─────────────────────────────────

    #[test]
    fn test_resolve_exec_cwd_absolute_inside_workdir() {
        let tmp = setup_workdir();
        let wd = tmp.path();
        let abs = wd.join("src");
        let resolved = resolve_exec_cwd(&abs.to_string_lossy(), Some(wd)).unwrap();
        assert!(resolved.starts_with(normalize_path(wd)));
    }

    #[test]
    fn test_resolve_exec_cwd_absolute_forward_slashes() {
        let tmp = setup_workdir();
        let wd = tmp.path();
        let abs = wd.join("src").to_string_lossy().replace('\\', "/");
        let resolved = resolve_exec_cwd(&abs, Some(wd)).unwrap();
        assert!(resolved.starts_with(normalize_path(wd)));
    }

    #[test]
    fn test_resolve_exec_cwd_absolute_outside_workdir_rejected() {
        let tmp = setup_workdir();
        let outside = std::env::temp_dir().join("definitely-not-inside");
        let err = resolve_exec_cwd(&outside.to_string_lossy(), Some(tmp.path()));
        assert!(err.is_err());
        assert_eq!(err.unwrap_err().code, -32602);
    }

    #[test]
    fn test_resolve_exec_cwd_relative_still_sandboxed() {
        let tmp = setup_workdir();
        let err = resolve_exec_cwd("../../elsewhere", Some(tmp.path()));
        assert!(err.is_err());
    }

    // ── helpers ──────────────────────────────────────────

    #[test]
    fn test_match_glob_star() {
        assert!(match_glob("*", "anything.txt"));
    }

    #[test]
    fn test_match_glob_extension() {
        assert!(match_glob("*.rs", "main.rs"));
        assert!(!match_glob("*.rs", "main.ts"));
    }

    #[test]
    fn test_match_glob_case_insensitive() {
        assert!(match_glob("*.RS", "main.rs"));
    }

    #[test]
    fn test_infer_mime_type() {
        assert_eq!(infer_mime_type("file.rs"), "text/x-rust");
        assert_eq!(infer_mime_type("file.png"), "image/png");
        assert_eq!(infer_mime_type("file.json"), "application/json");
        assert_eq!(
            infer_mime_type("file.unknownext"),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_simple_base64_encode() {
        assert_eq!(simple_base64_encode(b"Hello"), "SGVsbG8=");
        assert_eq!(simple_base64_encode(b"Hi"), "SGk=");
        assert_eq!(simple_base64_encode(b"H"), "SA==");
        assert_eq!(simple_base64_encode(b""), "");
    }

    #[test]
    fn test_decode_cursor_offset_none() {
        assert_eq!(decode_cursor_offset(None).unwrap(), 0);
    }

    #[test]
    fn test_decode_cursor_offset_invalid() {
        assert!(decode_cursor_offset(Some("invalid")).is_err());
        assert!(decode_cursor_offset(Some("zz")).is_err());
    }

    #[test]
    fn test_decode_cursor_offset_round_trip() {
        let cursor =
            crate::extensions::pagination::encode_cursor(serde_json::json!({"offset": 10}));
        assert_eq!(decode_cursor_offset(Some(&cursor)).unwrap(), 10);
    }

    #[test]
    fn test_is_executable_binary() {
        assert!(is_executable_binary("program.exe"));
        assert!(is_executable_binary("lib.so"));
        assert!(!is_executable_binary("main.rs"));
    }

    // ── no working directory ─────────────────────────────

    #[tokio::test]
    async fn test_list_no_working_dir() {
        let ctx = make_ctx_no_dir();
        let handler = FilesHandler::new();
        let result = handler.handle("list", serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32602);
    }
}
