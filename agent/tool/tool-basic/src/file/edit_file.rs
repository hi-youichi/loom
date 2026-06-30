//! Edit-file tool: performs exact string replacements in a file under the working folder.
//!
//! The fuzzy find-and-replace engine lives in [`loom_util::text`] and is
//! re-exported here as [`replace`] for backward compatibility.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};
use tool_core::Tool;

use super::path::resolve_path_under;

/// Tool name for editing a file.
pub use tool_core::tool_name::TOOL_EDIT_FILE;

/// Re-export of the fuzzy find-and-replace engine from `loom-core`.
///
/// Kept here so existing callers (`apply_patch`, `multiedit`, `file::mod`) can
/// continue to use `super::edit_file::replace` without changing their imports.
pub use loom_util::text::fuzzy_replace::replace;

const DESCRIPTION: &str = "\
Performs exact string replacements in files.

Usage:
- You must use your `read` tool at least once in the conversation before editing. \
This tool will error if you attempt an edit without reading the file.
- When editing text from read tool output, ensure you preserve the exact indentation \
(tabs/spaces) as it appears AFTER the line number prefix.
- ALWAYS prefer editing existing files in the codebase. NEVER write new files unless \
explicitly required.
- Only use emojis if the user explicitly requests it.
- The edit will FAIL if `oldString` is not found in the file.
- The edit will FAIL if `oldString` is found multiple times and you have not set \
`replaceAll`. Either provide more surrounding context in `oldString` to uniquely identify \
the match, or set `replaceAll` to true.
- Use `replaceAll` for renaming a variable or string across the entire file.";

/// Tool that performs exact string replacements in a file under the working folder.
///
/// Tries multiple matching strategies in priority order so that minor whitespace,
/// indentation, or escape-sequence differences between the LLM's proposed `oldString`
/// and the actual file do not block the edit.
pub struct EditFileTool {
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl EditFileTool {
    /// Creates a new EditFileTool with the given working folder.
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        TOOL_EDIT_FILE
    }

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
            name: TOOL_EDIT_FILE.to_string(),
            description: Some(DESCRIPTION.to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to working folder."
                    },
                    "oldString": {
                        "type": "string",
                        "description": "The text to replace."
                    },
                    "newString": {
                        "type": "string",
                        "description": "The text to replace it with (must differ from oldString)."
                    },
                    "replaceAll": {
                        "type": "boolean",
                        "description": "Replace all occurrences of oldString (default false).",
                        "default": false
                    }
                },
                "required": ["path", "oldString", "newString"]
            }),
            output_hint: None,
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
        let old_string = args
            .get("oldString")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing oldString".to_string()))?;
        let new_string = args
            .get("newString")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing newString".to_string()))?;
        let replace_all = args
            .get("replaceAll")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let path = resolve_path_under(self.working_folder.as_ref(), path_param)?;

        if old_string == new_string {
            return Ok(ToolCallContent::diff(
                path_param.to_string(),
                None,
                String::new(),
            ));
        }

        // Create / overwrite the file when oldString is empty (new file semantics).
        if old_string.is_empty() {
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        ToolSourceError::Transport(format!("failed to create parent dir: {}", e))
                    })?;
                }
            }
            std::fs::write(&path, new_string)
                .map_err(|e| ToolSourceError::Transport(format!("failed to write file: {}", e)))?;
            return Ok(ToolCallContent::diff(
                path_param.to_string(),
                None,
                new_string.to_string(),
            ));
        }

        if !path.exists() {
            return Err(ToolSourceError::InvalidInput(format!(
                "file not found: {}",
                path.display()
            )));
        }
        if path.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "path is a directory, not a file: {}",
                path.display()
            )));
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| ToolSourceError::Transport(format!("failed to read file: {}", e)))?;

        let new_content = replace(&content, old_string, new_string, replace_all)
            .map_err(ToolSourceError::InvalidInput)?;

        std::fs::write(&path, &new_content)
            .map_err(|e| ToolSourceError::Transport(format!("failed to write file: {}", e)))?;

        Ok(ToolCallContent::diff(
            path_param.to_string(),
            Some(content),
            new_content,
        ))
    }
}
