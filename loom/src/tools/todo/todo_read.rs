//! Todo-read tool: read todo list from XDG state home (e.g. ~/.local/state/loom/todos.json).
//!
//! Returns JSON array; returns empty array if file is missing or invalid.
//! Uses [`cross_xdg`] for path. Interacts with [`Tool`](crate::tools::Tool).
//! Effective definition for LLM comes from `loom/tools/todo_read.yaml` via [`YamlSpecToolSource`](crate::tool_source::YamlSpecToolSource) override.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::todo_file_path;
use super::TodoInfo;

/// Tool name for reading the todo list.
pub const TOOL_TODO_READ: &str = "todo_read";

/// Tool that reads the todo list from XDG state home.
///
/// Returns `[]` when the file does not exist or is invalid JSON.
pub struct TodoReadTool {
    /// Kept for registration compatibility with file tool source; path uses XDG, not this.
    #[allow(dead_code)]
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl TodoReadTool {
    /// Creates a new TodoReadTool with the given working folder.
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for TodoReadTool {
    fn name(&self) -> &str {
        TOOL_TODO_READ
    }

    /// Minimal spec; overridden by `loom/tools/todo_read.yaml` in YamlSpecToolSource.
    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_TODO_READ.to_string(),
            description: Some("Read the current todo list.".to_string()),
            input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
        }
    }

    async fn call(
        &self,
        _args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let path = todo_file_path()?;
        let todos: Vec<TodoInfo> = if path.exists() && path.is_file() {
            let s = std::fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&s).unwrap_or_default()
        } else {
            vec![]
        };
        let incomplete = todos
            .iter()
            .filter(|t| t.status != "completed")
            .count();
        let output = serde_json::to_string_pretty(&todos).unwrap_or_else(|_| "[]".to_string());
        Ok(ToolCallContent {
            text: format!("{} todos\n{}", incomplete, output),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::tools::Tool;

    use super::{TodoReadTool, TOOL_TODO_READ};

    /// TodoReadTool::name returns "todo_read".
    #[tokio::test]
    async fn todo_read_tool_name_returns_todo_read() {
        let tool = TodoReadTool::new(Arc::new(std::path::PathBuf::from("/")));
        assert_eq!(tool.name(), TOOL_TODO_READ);
    }

    /// TodoReadTool::spec has name and description and empty required.
    #[tokio::test]
    async fn todo_read_tool_spec_has_name_and_empty_schema() {
        let tool = TodoReadTool::new(Arc::new(std::path::PathBuf::from("/")));
        let spec = tool.spec();
        assert_eq!(spec.name, TOOL_TODO_READ);
        assert!(spec.description.as_ref().map_or(false, |d| d.contains("todo")));
        let required = spec.input_schema.get("required").and_then(serde_json::Value::as_array);
        assert!(required.map_or(true, |a| a.is_empty()));
    }

    /// When XDG todo file does not exist, call returns "0 todos" and "[]".
    #[tokio::test]
    async fn todo_read_call_when_file_missing_returns_empty_list() {
        let _g = crate::tools::todo::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_STATE_HOME", dir.path());
        let tool = TodoReadTool::new(Arc::new(dir.path().to_path_buf()));
        let out = tool.call(serde_json::json!({}), None).await.unwrap();
        assert!(out.text.starts_with("0 todos"));
        assert!(out.text.contains("[]"));
    }

    /// When XDG todo file exists with valid JSON, call returns count and list.
    #[tokio::test]
    async fn todo_read_call_when_file_exists_returns_parsed_todos() {
        let _g = crate::tools::todo::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_STATE_HOME", dir.path());
        let path = crate::tools::todo::todo_file_path().unwrap();
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        let todos = serde_json::json!([
            { "id": "1", "content": "Task one", "status": "pending", "priority": "high" },
            { "id": "2", "content": "Task two", "status": "completed", "priority": "medium" }
        ]);
        std::fs::write(&path, serde_json::to_string_pretty(&todos).unwrap()).unwrap();
        let tool = TodoReadTool::new(Arc::new(dir.path().to_path_buf()));
        let out = tool.call(serde_json::json!({}), None).await.unwrap();
        assert!(out.text.contains("1 todos")); // one incomplete
        assert!(out.text.contains("Task one"));
        assert!(out.text.contains("Task two"));
        assert!(out.text.contains("completed"));
    }

    /// When file exists but is invalid JSON, call returns empty list (default).
    #[tokio::test]
    async fn todo_read_call_when_invalid_json_returns_empty_list() {
        let _g = crate::tools::todo::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_STATE_HOME", dir.path());
        let path = crate::tools::todo::todo_file_path().unwrap();
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(&path, "not json").unwrap();
        let tool = TodoReadTool::new(Arc::new(dir.path().to_path_buf()));
        let out = tool.call(serde_json::json!({}), None).await.unwrap();
        assert!(out.text.starts_with("0 todos"));
        assert!(out.text.contains("[]"));
    }
}
