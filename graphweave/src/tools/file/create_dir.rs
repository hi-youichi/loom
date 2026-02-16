//! Create-dir tool: create a directory under the working folder.
//!
//! Exposes `create_dir` as a tool for the LLM. Creates parent directories if
//! needed. Path is validated to be under working folder. Interacts with
//! [`Tool`](crate::tools::Tool), [`ToolSpec`](crate::tool_source::ToolSpec).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::path::resolve_path_under;

/// Tool name for creating a directory.
pub const TOOL_CREATE_DIR: &str = "create_dir";

/// Tool that creates a directory under the working folder.
///
/// Parent directories are created if needed. With `exist_ok: true` (default),
/// no error when the directory already exists. Interacts with
/// [`resolve_path_under`] for path validation.
pub struct CreateDirTool {
    /// Canonical working folder path (shared with other file tools).
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl CreateDirTool {
    /// Creates a new CreateDirTool with the given working folder.
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for CreateDirTool {
    fn name(&self) -> &str {
        TOOL_CREATE_DIR
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_CREATE_DIR.to_string(),
            description: Some(
                "Create a directory; parent directories are created if needed. Path is relative \
                 to the working folder."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path relative to working folder."
                    },
                    "exist_ok": {
                        "type": "boolean",
                        "description": "If true, no error when directory already exists. Default true.",
                        "default": true
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
        let exist_ok = args
            .get("exist_ok")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let path = resolve_path_under(self.working_folder.as_ref(), path_param)?;
        if path.exists() {
            if path.is_dir() {
                if exist_ok {
                    return Ok(ToolCallContent {
                        text: "ok".to_string(),
                    });
                }
                return Err(ToolSourceError::InvalidInput(format!(
                    "directory already exists: {}",
                    path.display()
                )));
            }
            return Err(ToolSourceError::InvalidInput(format!(
                "path exists and is not a directory: {}",
                path.display()
            )));
        }
        std::fs::create_dir_all(&path).map_err(|e| {
            ToolSourceError::Transport(format!("failed to create directory: {}", e))
        })?;
        Ok(ToolCallContent {
            text: "ok".to_string(),
        })
    }
}
