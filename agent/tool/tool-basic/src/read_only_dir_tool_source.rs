//! Read-only directory connector: list_dir and read for a second root (no write/delete).
//!
//! Use as a connector example: expose a read-only view of a directory (e.g. reference docs,
//! cloud mount). Register with [`register_read_only_dir_tools`] on a [`ToolRegistryLocked`]
//! to provide read-only access alongside other tools.
//! Tool names are prefixed (`read_only_list_dir`, `read_only_read_file`) to avoid collision.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError, Tool, ToolRegistryLocked};
use crate::file::resolve_path_under;

pub const TOOL_READ_ONLY_LIST_DIR: &str = "read_only_list_dir";
pub const TOOL_READ_ONLY_READ_FILE: &str = "read_only_read_file";

/// Registers read-only directory tools on an existing [`ToolRegistryLocked`].
///
/// The path must exist and be a directory; it is canonicalized. Tools are
/// `read_only_list_dir` and `read_only_read` (path relative to this root).
///
/// # Errors
///
/// - [`ToolSourceError::InvalidInput`] if the path does not exist or is not a directory.
pub fn register_read_only_dir_tools(
    registry: &ToolRegistryLocked,
    read_only_root: impl AsRef<Path>,
) -> Result<(), ToolSourceError> {
    let path = read_only_root.as_ref();
    let canonical = path.canonicalize().map_err(|e| {
        ToolSourceError::InvalidInput(format!(
            "read_only root not found or not a directory: {}",
            e
        ))
    })?;

    if !canonical.is_dir() {
        return Err(ToolSourceError::InvalidInput(
            "read_only root is not a directory".to_string(),
        ));
    }

    let read_only_root = Arc::new(canonical);

    registry.register_sync(Box::new(ReadOnlyListDirTool::new(read_only_root.clone())));
    registry.register_sync(Box::new(ReadOnlyReadFileTool::new(read_only_root)));

    Ok(())
}

struct ReadOnlyListDirTool {
    read_only_root: Arc<std::path::PathBuf>,
}

impl ReadOnlyListDirTool {
    fn new(read_only_root: Arc<std::path::PathBuf>) -> Self {
        Self { read_only_root }
    }
}

#[async_trait]
impl Tool for ReadOnlyListDirTool {
    fn name(&self) -> &str {
        TOOL_READ_ONLY_LIST_DIR
    }

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
            name: TOOL_READ_ONLY_LIST_DIR.to_string(),
            description: Some(
                "List files and directories in a read-only root directory. \
                 Returns a tree structure with sizes and types."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path relative to the read-only root"
                    },
                    "ignore": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Optional glob patterns to ignore"
                    }
                }
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let resolved = resolve_path_under(&self.read_only_root, path)
            .map_err(|e| ToolSourceError::InvalidInput(format!("invalid path: {}", e)))?;

        let ignore_patterns: Option<Vec<String>> = args
            .get("ignore")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());

        let output = crate::file::ls_internal(&resolved, ignore_patterns.as_deref())
            .map_err(|e| ToolSourceError::Transport(format!("failed to list directory: {}", e)))?;

        Ok(ToolCallContent::text(output))
    }
}

struct ReadOnlyReadFileTool {
    read_only_root: Arc<std::path::PathBuf>,
}

impl ReadOnlyReadFileTool {
    fn new(read_only_root: Arc<std::path::PathBuf>) -> Self {
        Self { read_only_root }
    }
}

#[async_trait]
impl Tool for ReadOnlyReadFileTool {
    fn name(&self) -> &str {
        TOOL_READ_ONLY_READ_FILE
    }

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
            name: TOOL_READ_ONLY_READ_FILE.to_string(),
            description: Some(
                "Read a file from a read-only root directory. \
                 Path is relative to the read-only root."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path relative to the read-only root"
                    },
                    "encoding": {
                        "type": "string",
                        "description": "Optional encoding (default: utf-8)"
                    }
                },
                "required": ["path"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing 'path' field".to_string()))?;

        let resolved = resolve_path_under(&self.read_only_root, path)
            .map_err(|e| ToolSourceError::InvalidInput(format!("invalid path: {}", e)))?;

        let encoding = args.get("encoding").and_then(|v| v.as_str());

        let content = crate::file::read_file_internal(&resolved, encoding)
            .map_err(|e| ToolSourceError::Transport(format!("failed to read file: {}", e)))?;

        Ok(ToolCallContent::text(content))
    }
}