//! Write-file tool: write text content to a file under the working folder.
//!
//! Exposes `write_file` as a tool for the LLM. Creates parent directories if
//! needed. Path is validated to be under working folder. Interacts with
//! [`Tool`](crate::tools::Tool), [`ToolSpec`](crate::tool_source::ToolSpec).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::path::resolve_path_under;

/// Tool name for writing a file.
pub const TOOL_WRITE_FILE: &str = "write_file";

/// Tool that writes text content to a file under the working folder.
///
/// Creates parent directories if needed. Overwrites by default; optional append.
/// Interacts with [`resolve_path_under`] for path validation.
pub struct WriteFileTool {
    /// Canonical working folder path (shared with other file tools).
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl WriteFileTool {
    /// Creates a new WriteFileTool with the given working folder.
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        TOOL_WRITE_FILE
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_WRITE_FILE.to_string(),
            description: Some(
                "Write text content to a file. Creates parent directories if needed. Path is \
                 relative to the working folder. Overwrites if file exists."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to working folder."
                    },
                    "content": {
                        "type": "string",
                        "description": "Text content to write."
                    },
                    "append": {
                        "type": "boolean",
                        "description": "If true, append to existing file. Default false.",
                        "default": false
                    }
                },
                "required": ["path", "content"]
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
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing content".to_string()))?;
        let append = args
            .get("append")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let path = resolve_path_under(self.working_folder.as_ref(), path_param)?;
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ToolSourceError::Transport(format!("failed to create parent dir: {}", e))
                })?;
            }
        }
        let result = if append {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
        } else {
            std::fs::File::create(&path)
        };
        let mut f = result.map_err(|e| {
            ToolSourceError::Transport(format!("failed to open file for write: {}", e))
        })?;
        std::io::Write::write_all(&mut f, content.as_bytes())
            .map_err(|e| ToolSourceError::Transport(format!("failed to write file: {}", e)))?;
        Ok(ToolCallContent {
            text: "ok".to_string(),
        })
    }
}
