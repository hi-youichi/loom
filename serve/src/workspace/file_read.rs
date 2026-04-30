//! Handle `workspace_file_read` requests — read file content within a workspace.

use loom::protocol::requests::WorkspaceFileReadRequest;
use loom::protocol::responses::ServerResponse;
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) async fn handle_workspace_file_read(
    req: WorkspaceFileReadRequest,
    workspace_store: Option<Arc<loom_workspace::Store>>,
) -> ServerResponse {
    let _store = match workspace_store {
        Some(s) => s,
        None => return super::no_store_error(&req.id),
    };

    let root_dir = resolve_workspace_root(&req.workspace_id);
    let target = validate_and_resolve_file(&root_dir, &req.path);

    let target = match target {
        Ok(t) => t,
        Err(e) => {
            return ServerResponse::Error(loom::ErrorResponse {
                id: Some(req.id),
                error: e,
            })
        }
    };

    let content = match std::fs::read_to_string(&target) {
        Ok(c) => c,
        Err(e) => {
            return ServerResponse::Error(loom::ErrorResponse {
                id: Some(req.id),
                error: format!("failed to read file: {}", e),
            })
        }
    };

    ServerResponse::WorkspaceFileRead(loom::protocol::responses::WorkspaceFileReadResponse {
        id: req.id,
        workspace_id: req.workspace_id,
        path: req.path,
        content,
    })
}

fn resolve_workspace_root(workspace_id: &str) -> std::path::PathBuf {
    let base = std::env::var("WORKSPACE_ROOT_DIR")
        .ok()
        .unwrap_or_else(|| "workspaces".to_string());
    PathBuf::from(base).join(workspace_id)
}

fn validate_and_resolve_file(root: &std::path::Path, relative_path: &str) -> Result<PathBuf, String> {
    let canonical_root = root
        .canonicalize()
        .unwrap_or_else(|_| root.to_path_buf());

    let target = root.join(relative_path);

    let canonical_target = target
        .canonicalize()
        .map_err(|e| format!("path not found: {}", e))?;

    if !canonical_target.starts_with(&canonical_root) {
        return Err("path traversal detected".to_string());
    }

    if !canonical_target.is_file() {
        return Err("path is not a file".to_string());
    }

    Ok(canonical_target)
}
