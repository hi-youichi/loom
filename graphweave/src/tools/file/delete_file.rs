//! Delete-file tool: delete a file or directory under the working folder.
//!
//! Exposes `delete_file` as a tool for the LLM. Path is validated to be under
//! working folder. Non-empty directories require `recursive: true`. Interacts
//! with [`Tool`](crate::tools::Tool), [`ToolSpec`](crate::tool_source::ToolSpec).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::path::resolve_path_under;

/// Tool name for deleting a file or directory.
pub const TOOL_DELETE_FILE: &str = "delete_file";

/// Tool that deletes a file or directory under the working folder.
///
/// For non-empty directories, use `recursive: true`. Interacts with
/// [`resolve_path_under`] for path validation.
pub struct DeleteFileTool {
    /// Canonical working folder path (shared with other file tools).
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl DeleteFileTool {
    /// Creates a new DeleteFileTool with the given working folder.
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for DeleteFileTool {
    fn name(&self) -> &str {
        TOOL_DELETE_FILE
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_DELETE_FILE.to_string(),
            description: Some(
                "Delete a file or an empty directory. Path must be under the working folder. \
                 For non-empty directories, use recursive option with caution."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File or directory path relative to working folder."
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "If true, remove directory and its contents. Default false.",
                        "default": false
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let path_param = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing path".to_string()))?;
        let recursive = args
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let path = resolve_path_under(self.working_folder.as_ref(), path_param)?;
        if !path.exists() {
            return Err(ToolSourceError::NotFound(format!(
                "path not found: {}",
                path.display()
            )));
        }
        if path.is_dir() {
            if recursive {
                std::fs::remove_dir_all(&path).map_err(|e| {
                    ToolSourceError::Transport(format!("failed to remove directory: {}", e))
                })?;
            } else {
                match std::fs::remove_dir(&path) {
                    Ok(()) => {}
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::DirectoryNotEmpty {
                            return Err(ToolSourceError::InvalidInput(format!(
                                "directory not empty: {}",
                                path.display()
                            )));
                        }
                        return Err(ToolSourceError::Transport(format!(
                            "failed to remove directory: {}",
                            e
                        )));
                    }
                }
            }
        } else {
            std::fs::remove_file(&path)
                .map_err(|e| ToolSourceError::Transport(format!("failed to remove file: {}", e)))?;
        }
        Ok(ToolCallContent {
            text: "ok".to_string(),
        })
    }
}
