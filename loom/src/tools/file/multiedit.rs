//! Multi-edit tool: apply multiple find-and-replace operations to a single file in one call.
//!
//! Uses the same replacement logic as [`EditFileTool`]. All edits are applied in sequence;
//! if any edit fails, none are applied (atomic).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::edit_file::replace as edit_replace;
use super::path::resolve_path_under;

/// Tool name for multi-edit.
pub const TOOL_MULTIEDIT: &str = "multiedit";

/// Tool that applies multiple edits to one file in a single call.
pub struct MultieditTool {
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl MultieditTool {
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for MultieditTool {
    fn name(&self) -> &str {
        TOOL_MULTIEDIT
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_MULTIEDIT.to_string(),
            description: Some(
                "Apply multiple find-and-replace edits to a single file in one call. \
                 Edits are applied in order; all or none (atomic). Use Read first."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to working folder."
                    },
                    "edits": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "oldString": { "type": "string" },
                                "newString": { "type": "string" },
                                "replaceAll": { "type": "boolean", "default": false }
                            },
                            "required": ["oldString", "newString"]
                        },
                        "description": "List of edits to apply in order."
                    }
                },
                "required": ["path", "edits"]
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

        let edits = args
            .get("edits")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing or invalid edits array".to_string()))?;

        if edits.is_empty() {
            return Err(ToolSourceError::InvalidInput("edits must not be empty".to_string()));
        }

        let mut content = if path.exists() && !path.is_dir() {
            std::fs::read_to_string(&path)
                .map_err(|e| ToolSourceError::Transport(format!("failed to read file: {}", e)))?
        } else if path.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "path is a directory: {}",
                path.display()
            )));
        } else {
            // New file: first edit must have empty oldString, newString = initial content
            let first = edits[0].as_object().ok_or_else(|| {
                ToolSourceError::InvalidInput("each edit must be an object".to_string())
            })?;
            let old = first.get("oldString").and_then(|v| v.as_str()).unwrap_or("");
            let new = first.get("newString").and_then(|v| v.as_str()).unwrap_or("");
            if !old.is_empty() {
                return Err(ToolSourceError::InvalidInput(
                    "file does not exist; first edit must have empty oldString with newString as full content".to_string(),
                ));
            }
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        ToolSourceError::Transport(format!("failed to create parent dir: {}", e))
                    })?;
                }
            }
            let mut new_content = new.to_string();
            for (i, ed) in edits.iter().enumerate().skip(1) {
                let obj = ed.as_object().ok_or_else(|| {
                    ToolSourceError::InvalidInput("each edit must be an object".to_string())
                })?;
                let old_s = obj.get("oldString").and_then(|v| v.as_str()).unwrap_or("");
                let new_s = obj.get("newString").and_then(|v| v.as_str()).unwrap_or("");
                let replace_all = obj.get("replaceAll").and_then(|v| v.as_bool()).unwrap_or(false);
                if old_s == new_s {
                    return Err(ToolSourceError::InvalidInput(format!(
                        "edit {}: oldString and newString must differ",
                        i + 1
                    )));
                }
                new_content = edit_replace(&new_content, old_s, new_s, replace_all)
                    .map_err(|e| ToolSourceError::InvalidInput(format!("edit {}: {}", i + 1, e)))?;
            }
            std::fs::write(&path, &new_content).map_err(|e| {
                ToolSourceError::Transport(format!("failed to write file: {}", e))
            })?;
            return Ok(ToolCallContent {
                text: format!("Created file with {} edit(s).", edits.len()),
            });
        };

        for (i, ed) in edits.iter().enumerate() {
            let obj = ed.as_object().ok_or_else(|| {
                ToolSourceError::InvalidInput("each edit must be an object".to_string())
            })?;
            let old_s = obj.get("oldString").and_then(|v| v.as_str()).unwrap_or("");
            let new_s = obj.get("newString").and_then(|v| v.as_str()).unwrap_or("");
            let replace_all = obj.get("replaceAll").and_then(|v| v.as_bool()).unwrap_or(false);
            if old_s == new_s {
                return Err(ToolSourceError::InvalidInput(format!(
                    "edit {}: oldString and newString must differ",
                    i + 1
                )));
            }
            content = edit_replace(&content, old_s, new_s, replace_all)
                .map_err(|e| ToolSourceError::InvalidInput(format!("edit {}: {}", i + 1, e)))?;
        }

        std::fs::write(&path, &content)
            .map_err(|e| ToolSourceError::Transport(format!("failed to write file: {}", e)))?;

        Ok(ToolCallContent {
            text: format!("Applied {} edit(s) successfully.", edits.len()),
        })
    }
}
