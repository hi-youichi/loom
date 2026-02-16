//! List-directory tool: list entries in a directory under the working folder.
//!
//! Exposes `list_dir` as a tool for the LLM. Path is validated to be under
//! working folder. Interacts with [`Tool`](crate::tools::Tool), [`ToolSpec`](crate::tool_source::ToolSpec).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::path::resolve_path_under;

/// Tool name for listing directory entries.
pub const TOOL_LIST_DIR: &str = "list_dir";

/// Tool that lists files and subdirectories in a directory under the working folder.
///
/// Path is relative to working folder; use "." for the root. Interacts with
/// [`resolve_path_under`] for path validation.
pub struct ListDirTool {
    /// Canonical working folder path (shared with other file tools).
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl ListDirTool {
    /// Creates a new ListDirTool with the given working folder.
    ///
    /// The path is not canonicalized here; the caller must pass a canonical path
    /// (e.g. from [`FileToolSource::new`](crate::tool_source::FileToolSource::new)).
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        TOOL_LIST_DIR
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_LIST_DIR.to_string(),
            description: Some(
                "List entries (files and subdirectories) in a directory. Path is relative to the \
                 working folder. Use '.' for the working folder root."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path relative to working folder (use '.' for working folder root)."
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
        let path_param = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let dir = resolve_path_under(self.working_folder.as_ref(), path_param)?;
        if !dir.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "not a directory: {}",
                dir.display()
            )));
        }
        let mut entries: Vec<String> = std::fs::read_dir(&dir)
            .map_err(|e| ToolSourceError::Transport(format!("failed to read dir: {}", e)))?
            .filter_map(|e| e.ok())
            .map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let kind = if e.path().is_dir() { "dir" } else { "file" };
                format!("{} ({})", name, kind)
            })
            .collect();
        entries.sort();
        Ok(ToolCallContent {
            text: entries.join("\n"),
        })
    }
}
