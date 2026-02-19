//! Read-file tool: read text content of a file under the working folder.
//!
//! Exposes `read` as a tool for the LLM. Path is validated to be under
//! working folder. Supports offset/limit for long files. Interacts with
//! [`Tool`](crate::tools::Tool), [`ToolSpec`](crate::tool_source::ToolSpec).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::path::resolve_path_under;

/// Tool name for reading a file.
pub const TOOL_READ_FILE: &str = "read";

const DEFAULT_READ_LIMIT: usize = 2000;
const MAX_LINE_LENGTH: usize = 2000;

/// Tool that reads text content of a file under the working folder.
///
/// Supports offset (0-based line index) and limit. Uses UTF-8; lines longer
/// than MAX_LINE_LENGTH are truncated. Output format: "  {line_num}\t{content}".
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
                "Read file content. Path relative to working folder. Optional offset (0-based) and limit (default 2000). \
                 Output in cat -n style with line numbers."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to working folder."
                    },
                    "offset": {
                        "type": "integer",
                        "description": "0-based line number to start reading from.",
                        "minimum": 0
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max lines to read (default 2000).",
                        "minimum": 1,
                        "default": 2000
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

        let offset = args
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(0);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_READ_LIMIT);

        let lines: Vec<&str> = content.split('\n').collect();
        let total = lines.len();
        let start = offset.min(total);
        let end = (start + limit).min(total);
        let selected = &lines[start..end];

        let mut out = String::new();
        for (i, line) in selected.iter().enumerate() {
            let line_num = start + i + 1;
            let truncated = if line.len() > MAX_LINE_LENGTH {
                format!("{}...", &line[..MAX_LINE_LENGTH])
            } else {
                (*line).to_string()
            };
            out.push_str(&format!("  {}\t{}\n", line_num, truncated));
        }
        Ok(ToolCallContent { text: out })
    }
}
