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
        let path = resolve_path_under(self.working_folder.as_ref(), path_param)?;

        let edits = args
            .get("edits")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ToolSourceError::InvalidInput("missing or invalid edits array".to_string())
            })?;

        if edits.is_empty() {
            return Err(ToolSourceError::InvalidInput(
                "edits must not be empty".to_string(),
            ));
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
            let old = first
                .get("oldString")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let new = first
                .get("newString")
                .and_then(|v| v.as_str())
                .unwrap_or("");
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
                let replace_all = obj
                    .get("replaceAll")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if old_s == new_s {
                    return Err(ToolSourceError::InvalidInput(format!(
                        "edit {}: oldString and newString must differ",
                        i + 1
                    )));
                }
                new_content = edit_replace(&new_content, old_s, new_s, replace_all)
                    .map_err(|e| ToolSourceError::InvalidInput(format!("edit {}: {}", i + 1, e)))?;
            }
            std::fs::write(&path, &new_content)
                .map_err(|e| ToolSourceError::Transport(format!("failed to write file: {}", e)))?;
            return Ok(ToolCallContent::text(format!(
                "Created file with {} edit(s).",
                edits.len()
            )));
        };

        for (i, ed) in edits.iter().enumerate() {
            let obj = ed.as_object().ok_or_else(|| {
                ToolSourceError::InvalidInput("each edit must be an object".to_string())
            })?;
            let old_s = obj.get("oldString").and_then(|v| v.as_str()).unwrap_or("");
            let new_s = obj.get("newString").and_then(|v| v.as_str()).unwrap_or("");
            let replace_all = obj
                .get("replaceAll")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
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

        Ok(ToolCallContent::text(format!(
            "Applied {} edit(s) successfully.",
            edits.len()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_source::ToolSourceError;
    use serde_json::json;
    use std::sync::Arc;

    fn tool(dir: &tempfile::TempDir) -> MultieditTool {
        MultieditTool::new(Arc::new(dir.path().to_path_buf()))
    }

    #[test]
    fn multiedit_name_returns_tool_name() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(tool(&dir).name(), TOOL_MULTIEDIT);
    }

    #[test]
    fn multiedit_spec_has_required_fields() {
        let dir = tempfile::tempdir().unwrap();
        let spec = tool(&dir).spec();
        assert_eq!(spec.name, TOOL_MULTIEDIT);
        assert!(spec.description.is_some());
        assert!(spec.input_schema.get("properties").is_some());
    }

    #[tokio::test]
    async fn multiedit_call_missing_path_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        let err = t
            .call(
                json!({ "edits": [{ "oldString": "", "newString": "x" }] }),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn multiedit_call_path_not_string_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        let err = t
            .call(
                json!({ "path": 123, "edits": [{ "oldString": "", "newString": "x" }] }),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn multiedit_call_missing_edits_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        let err = t.call(json!({ "path": "f.txt" }), None).await.unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn multiedit_call_edits_not_array_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        let err = t
            .call(json!({ "path": "f.txt", "edits": "not array" }), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn multiedit_call_empty_edits_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "x").unwrap();
        let t = tool(&dir);
        let err = t
            .call(json!({ "path": "f.txt", "edits": [] }), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn multiedit_call_path_is_directory_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        let t = tool(&dir);
        let err = t
            .call(
                json!({
                    "path": "subdir",
                    "edits": [{ "oldString": "", "newString": "x" }]
                }),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn multiedit_call_new_file_first_edit_not_object_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        let err = t
            .call(
                json!({
                    "path": "new.txt",
                    "edits": ["not an object"]
                }),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn multiedit_call_new_file_first_edit_non_empty_old_string_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        let err = t
            .call(
                json!({
                    "path": "new.txt",
                    "edits": [{ "oldString": "x", "newString": "y" }]
                }),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn multiedit_call_new_file_multiple_edits_old_eq_new_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        let err = t
            .call(
                json!({
                    "path": "new.txt",
                    "edits": [
                        { "oldString": "", "newString": "initial" },
                        { "oldString": "x", "newString": "x" }
                    ]
                }),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn multiedit_call_new_file_single_edit_success() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        let result = t
            .call(
                json!({
                    "path": "new.txt",
                    "edits": [{ "oldString": "", "newString": "hello" }]
                }),
                None,
            )
            .await
            .unwrap();
        assert!(result.as_text().unwrap().contains("Created"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("new.txt")).unwrap(),
            "hello"
        );
    }

    #[tokio::test]
    async fn multiedit_call_new_file_in_subdir_creates_parent() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool(&dir);
        let result = t
            .call(
                json!({
                    "path": "a/b/new.txt",
                    "edits": [{ "oldString": "", "newString": "content" }]
                }),
                None,
            )
            .await
            .unwrap();
        assert!(result.as_text().unwrap().contains("Created"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a/b/new.txt")).unwrap(),
            "content"
        );
    }

    #[tokio::test]
    async fn multiedit_call_existing_file_edit_not_object_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "x").unwrap();
        let t = tool(&dir);
        let err = t
            .call(
                json!({
                    "path": "f.txt",
                    "edits": [{ "oldString": "x", "newString": "y" }, "not object"]
                }),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn multiedit_call_existing_file_old_eq_new_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "abc").unwrap();
        let t = tool(&dir);
        let err = t
            .call(
                json!({
                    "path": "f.txt",
                    "edits": [{ "oldString": "b", "newString": "b" }]
                }),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn multiedit_call_existing_file_old_not_found_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "original").unwrap();
        let t = tool(&dir);
        let err = t
            .call(
                json!({
                    "path": "f.txt",
                    "edits": [{ "oldString": "not_in_file", "newString": "y" }]
                }),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
            "original"
        );
    }

    #[tokio::test]
    async fn multiedit_call_existing_file_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "a\nb\nc").unwrap();
        let t = tool(&dir);
        let result = t
            .call(
                json!({
                    "path": "f.txt",
                    "edits": [
                        { "oldString": "a", "newString": "A" },
                        { "oldString": "c", "newString": "C" }
                    ]
                }),
                None,
            )
            .await
            .unwrap();
        assert!(result.as_text().unwrap().contains("Applied"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
            "A\nb\nC"
        );
    }
}
