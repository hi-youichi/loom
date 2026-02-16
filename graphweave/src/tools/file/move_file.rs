//! Move-file tool: move or rename a file or directory under the working folder.
//!
//! Exposes `move_file` as a tool for the LLM. Both source and target are
//! validated to be under working folder. Interacts with [`Tool`](crate::tools::Tool),
//! [`ToolSpec`](crate::tool_source::ToolSpec).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::path::resolve_path_under;

/// Tool name for moving or renaming a file or directory.
pub const TOOL_MOVE_FILE: &str = "move_file";

/// Tool that moves or renames a file or directory under the working folder.
///
/// Both source and target must be under the working folder. Interacts with
/// [`resolve_path_under`] for path validation.
pub struct MoveFileTool {
    /// Canonical working folder path (shared with other file tools).
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl MoveFileTool {
    /// Creates a new MoveFileTool with the given working folder.
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for MoveFileTool {
    fn name(&self) -> &str {
        TOOL_MOVE_FILE
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_MOVE_FILE.to_string(),
            description: Some(
                "Move or rename a file or directory. Both source and target must be under the \
                 working folder."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "description": "Source path relative to working folder."
                    },
                    "target": {
                        "type": "string",
                        "description": "Target path relative to working folder."
                    }
                },
                "required": ["source", "target"]
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let source_param = args
            .get("source")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing source".to_string()))?;
        let target_param = args
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing target".to_string()))?;
        let source = resolve_path_under(self.working_folder.as_ref(), source_param)?;
        let target = resolve_path_under(self.working_folder.as_ref(), target_param)?;
        if !source.exists() {
            return Err(ToolSourceError::NotFound(format!(
                "source not found: {}",
                source.display()
            )));
        }
        std::fs::rename(&source, &target)
            .map_err(|e| ToolSourceError::Transport(format!("failed to move: {}", e)))?;
        Ok(ToolCallContent {
            text: "ok".to_string(),
        })
    }
}
