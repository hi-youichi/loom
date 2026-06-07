//! Handle `workspace_file_list` requests — list directory entries within a workspace.

    use loom_protocol::requests::WorkspaceFileListRequest;
    use loom_protocol::responses::{FileEntry, ServerResponse, WorkspaceFileListResponse};
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) async fn handle_workspace_file_list(
    req: WorkspaceFileListRequest,
    workspace_store: Option<Arc<loom_workspace::Store>>,
) -> ServerResponse {
    let _store = match workspace_store {
        Some(s) => s,
        None => return super::no_store_error(&req.id),
    };

    let root_dir = resolve_workspace_root(&req.workspace_id);
    let relative_path = req.path.as_deref().unwrap_or("");
    let target_dir = validate_and_resolve(&root_dir, relative_path);

    let target_dir = match target_dir {
        Ok(d) => d,
        Err(e) => {
            return ServerResponse::Error(loom_protocol::ErrorResponse {
                id: Some(req.id),
                error: e,
            })
        }
    };

    let entries = match list_dir_entries(&target_dir, &root_dir) {
        Ok(e) => e,
        Err(e) => {
            return ServerResponse::Error(loom_protocol::ErrorResponse {
                id: Some(req.id),
                error: e.to_string(),
            })
        }
    };

    ServerResponse::WorkspaceFileList(WorkspaceFileListResponse {
        id: req.id,
        workspace_id: req.workspace_id,
        path: relative_path.to_string(),
        entries,
    })
}

/// Resolve workspace root directory from env var or default.
fn resolve_workspace_root(workspace_id: &str) -> std::path::PathBuf {
    let base = std::env::var("WORKSPACE_ROOT_DIR")
        .ok()
        .unwrap_or_else(|| "workspaces".to_string());
    PathBuf::from(base).join(workspace_id)
}

/// Validate that the resolved path stays within the workspace root.
fn validate_and_resolve(root: &std::path::Path, relative_path: &str) -> Result<PathBuf, String> {
    let canonical_root = root
        .canonicalize()
        .unwrap_or_else(|_| root.to_path_buf());

    let target = if relative_path.is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative_path)
    };

    let canonical_target = target
        .canonicalize()
        .map_err(|e| format!("path not found: {}", e))?;

    if !canonical_target.starts_with(&canonical_root) {
        return Err("path traversal detected".to_string());
    }

    if !canonical_target.is_dir() {
        return Err("path is not a directory".to_string());
    }

    Ok(canonical_target)
}

/// List directory entries, sorted: folders first, then files, alphabetically.
fn list_dir_entries(
    dir: &std::path::Path,
    _root: &std::path::Path,
) -> Result<Vec<FileEntry>, Box<dyn std::error::Error>> {
    let mut folders = Vec::new();
    let mut files = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files/dirs
        if name.starts_with('.') {
            continue;
        }

        let metadata = entry.metadata()?;
        let path = entry
            .path()
            .strip_prefix(dir)
            .unwrap_or(&entry.path())
            .to_string_lossy()
            .to_string();

        if metadata.is_dir() {
            folders.push(FileEntry {
                name,
                kind: "folder".to_string(),
                path,
                extension: None,
                size: None,
            });
        } else {
            let extension = std::path::Path::new(&name)
                .extension()
                .map(|e| e.to_string_lossy().to_string());
            files.push(FileEntry {
                name,
                kind: "file".to_string(),
                path,
                extension,
                size: Some(metadata.len()),
            });
        }
    }

    folders.sort_by_cached_key(|a| a.name.to_lowercase());
    files.sort_by_cached_key(|a| a.name.to_lowercase());

    let mut entries = folders;
    entries.extend(files);
    Ok(entries)
}
