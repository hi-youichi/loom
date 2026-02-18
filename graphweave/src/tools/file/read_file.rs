//! Read-file tool: read text content of a file under the working folder.
//!
//! Exposes `read` as a tool for the LLM. Path is validated to be under
//! working folder. Interacts with [`Tool`](crate::tools::Tool), [`ToolSpec`](crate::tool_source::ToolSpec).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::path::resolve_path_under;

/// Tool name for reading a file.
pub const TOOL_READ_FILE: &str = "read";

/// Tool that reads the entire text content of a file under the working folder.
///
/// Uses UTF-8 by default; optional encoding parameter is accepted but only UTF-8
/// is implemented. Interacts with [`resolve_path_under`] for path validation.
pub struct ReadFileTool {
    /// Canonical working folder path (shared with other file tools).
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl ReadFileTool {
    /// Creates a new ReadFileTool with the given working folder.
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        TOOL_READ_FILE
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_READ_FILE.to_string(),
            description: Some(
                "Read entire text content of a file. Path is relative to the working folder. \
                 Use for text files; binary files may be truncated or unsupported."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to working folder."
                    },
                    "encoding": {
                        "type": "string",
                        "description": "Optional encoding (e.g. 'utf-8'). Default utf-8.",
                        "default": "utf-8"
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
        let path = resolve_path_under(self.working_folder.as_ref(), path_param)?;
        if !path.exists() {
            return Err(ToolSourceError::InvalidInput(format!(
                "file not found: {}",
                path.display()
            )));
        }
        if path.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "is a directory, not a file: {}",
                path.display()
            )));
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| ToolSourceError::Transport(format!("failed to read file: {}", e)))?;
        Ok(ToolCallContent { text: content })
    }
}
