//! Todo-write tool: write todo list to thread data (~/.loom/thread/{thread_id}/todo.json).
//!
//! Accepts a full list of todos (id, content, status, priority); writes JSON.
//! Uses thread_id from ToolCallContext.config.thread_id for isolation.
//! Effective definition for LLM comes from `loom/tools/todo_write.yaml` via [`YamlSpecToolSource`](crate::tool_source::YamlSpecToolSource) override.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::todo_file_path;
use super::TodoInfo;

/// Tool name for writing the todo list.
pub const TOOL_TODO_WRITE: &str = "todo_write";

/// Tool that writes the todo list to thread context.
///
/// Path is `~/.loom/thread/{thread_id}/todo.json` when thread_id is provided;
/// falls back to `~/.loom/todo.json` for backward compatibility.
/// Creates parent dirs if needed.
pub struct TodoWriteTool {
    /// Kept for registration compatibility with file tool source; path uses XDG, not this.
    #[allow(dead_code)]
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl TodoWriteTool {
    /// Creates a new TodoWriteTool with the given working folder.
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

fn parse_todos(args: &serde_json::Value) -> Result<Vec<TodoInfo>, ToolSourceError> {
    let arr = args
        .get("todos")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            ToolSourceError::InvalidInput("missing or invalid 'todos' array".to_string())
        })?;
    let mut out = Vec::with_capacity(arr.len());
    for (i, v) in arr.iter().enumerate() {
        let obj = v.as_object().ok_or_else(|| {
            ToolSourceError::InvalidInput(format!("todos[{}] must be an object", i))
        })?;
        let id = obj
            .get("id")
            .and_then(|x| x.as_str())
            .map(String::from)
            .ok_or_else(|| ToolSourceError::InvalidInput(format!("todos[{}] missing 'id'", i)))?;
        let content = obj
            .get("content")
            .and_then(|x| x.as_str())
            .map(String::from)
            .ok_or_else(|| {
                ToolSourceError::InvalidInput(format!("todos[{}] missing 'content'", i))
            })?;
        let status = obj
            .get("status")
            .and_then(|x| x.as_str())
            .map(String::from)
            .unwrap_or_else(|| "pending".to_string());
        let priority = obj
            .get("priority")
            .and_then(|x| x.as_str())
            .map(String::from)
            .unwrap_or_else(|| "medium".to_string());
        out.push(TodoInfo {
            id,
            content,
            status,
            priority,
        });
    }
    Ok(out)
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        TOOL_TODO_WRITE
    }

    /// Minimal spec; overridden by `loom/tools/todo_write.yaml` in YamlSpecToolSource.
    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_TODO_WRITE.to_string(),
            description: Some("Write or replace the todo list.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": { "todos": { "type": "array" } },
                "required": ["todos"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let todos = parse_todos(&args)?;
        let thread_id = ctx.and_then(|c| c.thread_id.as_deref());
        let path = todo_file_path(thread_id)?;
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ToolSourceError::Transport(format!("failed to create parent dir: {}", e))
                })?;
            }
        }
        let json_bytes = serde_json::to_string_pretty(&todos)
            .map_err(|e| ToolSourceError::Transport(format!("failed to serialize todos: {}", e)))?;
        std::fs::write(&path, json_bytes).map_err(|e| {
            ToolSourceError::Transport(format!("failed to write {}: {}", path.display(), e))
        })?;
        let incomplete = todos.iter().filter(|t| t.status != "completed").count();
        let output = serde_json::to_string_pretty(&todos).unwrap_or_else(|_| "[]".to_string());
        Ok(ToolCallContent::text(format!(
            "{} todos\n{}",
            incomplete, output
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::tool_source::{ToolCallContext, ToolSourceError};
    use crate::tools::Tool;

    use super::{TodoWriteTool, TOOL_TODO_WRITE};

    /// TodoWriteTool::name returns "todo_write".
    #[tokio::test]
    async fn todo_write_tool_name_returns_todo_write() {
        let tool = TodoWriteTool::new(Arc::new(std::path::PathBuf::from("/")));
        assert_eq!(tool.name(), TOOL_TODO_WRITE);
    }

    /// TodoWriteTool::spec has name, description, and required "todos".
    #[tokio::test]
    async fn todo_write_tool_spec_has_todos_required() {
        let tool = TodoWriteTool::new(Arc::new(std::path::PathBuf::from("/")));
        let spec = tool.spec();
        assert_eq!(spec.name, TOOL_TODO_WRITE);
        assert!(spec
            .description
            .as_ref()
            .is_some_and(|d| d.contains("todo") || d.contains("Write")));
        let required = spec
            .input_schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(required.contains(&serde_json::json!("todos")));
    }

    /// call with valid todos writes file and returns count and list.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn todo_write_call_valid_todos_writes_and_returns() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = crate::tools::todo::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_HOME", dir.path());
        let tool = TodoWriteTool::new(Arc::new(dir.path().to_path_buf()));
        let args = serde_json::json!({
            "todos": [
                { "id": "a", "content": "First", "status": "pending", "priority": "high" },
                { "id": "b", "content": "Second", "status": "completed", "priority": "medium" }
            ]
        });
        let out = tool.call(args, None).await.unwrap();
        assert!(out.as_text().unwrap().contains("1 todos"));
        assert!(out.as_text().unwrap().contains("First"));
        assert!(out.as_text().unwrap().contains("Second"));
        let path = crate::tools::todo::todo_file_path(None).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("First"));
        assert!(raw.contains("completed"));
    }

    /// call with missing "todos" returns InvalidInput.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn todo_write_call_missing_todos_returns_invalid_input() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = crate::tools::todo::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_HOME", dir.path());
        let tool = TodoWriteTool::new(Arc::new(dir.path().to_path_buf()));
        let result = tool.call(serde_json::json!({}), None).await;
        let err = result.unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
        assert!(err.to_string().to_lowercase().contains("todos"));
    }

    /// call with todos not an array returns InvalidInput.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn todo_write_call_todos_not_array_returns_invalid_input() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = crate::tools::todo::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_HOME", dir.path());
        let tool = TodoWriteTool::new(Arc::new(dir.path().to_path_buf()));
        let result = tool
            .call(serde_json::json!({ "todos": "not array" }), None)
            .await;
        let err = result.unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    /// call with item missing "id" returns InvalidInput.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn todo_write_call_item_missing_id_returns_invalid_input() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = crate::tools::todo::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_HOME", dir.path());
        let tool = TodoWriteTool::new(Arc::new(dir.path().to_path_buf()));
        let result = tool
            .call(
                serde_json::json!({ "todos": [{ "content": "x", "status": "pending", "priority": "medium" }] }),
                None,
            )
            .await;
        let err = result.unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
        assert!(err.to_string().contains("id"));
    }

    /// call with item missing "content" returns InvalidInput.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn todo_write_call_item_missing_content_returns_invalid_input() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = crate::tools::todo::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_HOME", dir.path());
        let tool = TodoWriteTool::new(Arc::new(dir.path().to_path_buf()));
        let result = tool
            .call(
                serde_json::json!({ "todos": [{ "id": "1", "status": "pending", "priority": "medium" }] }),
                None,
            )
            .await;
        let err = result.unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
        assert!(err.to_string().contains("content"));
    }

    /// call with item as non-object returns InvalidInput.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn todo_write_call_item_not_object_returns_invalid_input() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = crate::tools::todo::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_HOME", dir.path());
        let tool = TodoWriteTool::new(Arc::new(dir.path().to_path_buf()));
        let result = tool
            .call(serde_json::json!({ "todos": ["string item"] }), None)
            .await;
        let err = result.unwrap_err();
        assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    }

    /// call with optional status/priority uses defaults (pending, medium).
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn todo_write_call_default_status_and_priority() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = crate::tools::todo::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_HOME", dir.path());
        let tool = TodoWriteTool::new(Arc::new(dir.path().to_path_buf()));
        let args = serde_json::json!({
            "todos": [{ "id": "1", "content": "Only required" }]
        });
        let out = tool.call(args, None).await.unwrap();
        assert!(out.as_text().unwrap().contains("1 todos"));
        let path = crate::tools::todo::todo_file_path(None).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("pending"));
        assert!(raw.contains("medium"));
    }

    /// call with thread_id writes to thread-specific path.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn todo_write_call_with_thread_id_writes_to_thread_path() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = crate::tools::todo::XDG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_HOME", dir.path());

        let thread_id = "thread-789";
        let tool = TodoWriteTool::new(Arc::new(dir.path().to_path_buf()));
        let args = serde_json::json!({
            "todos": [{ "id": "1", "content": "Thread task", "status": "pending", "priority": "high" }]
        });

        let ctx = ToolCallContext {
            thread_id: Some(thread_id.to_string()),
            ..Default::default()
        };

        let out = tool.call(args, Some(&ctx)).await.unwrap();
        assert!(out.as_text().unwrap().contains("1 todos"));

        // Verify file is in thread-specific path
        let thread_path = crate::tools::todo::todo_file_path(Some(thread_id)).unwrap();
        assert!(thread_path.exists());
        assert!(thread_path.to_str().unwrap().contains("thread"));
        assert!(thread_path.to_str().unwrap().contains(thread_id));

        let raw = std::fs::read_to_string(&thread_path).unwrap();
        assert!(raw.contains("Thread task"));
    }
}
