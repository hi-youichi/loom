//! BDD-style tests for todo_write and todo_read tools.
//!
//! Todo storage uses XDG state home; tests set XDG_STATE_HOME to a temp dir so the real home is not used.
//! Scenarios: todo_write then todo_read returns same list; todo_read when file missing returns empty;
//! todo_write with invalid payload returns InvalidInput.

mod init_logging;

use graphweave::tool_source::{FileToolSource, ToolSource, ToolSourceError};
use graphweave::tools::{TOOL_TODO_READ, TOOL_TODO_WRITE};
use serde_json::json;

/// Scenario: todo_write with valid todos then todo_read returns the same list (roundtrip).
#[tokio::test]
async fn todo_write_then_todo_read_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_STATE_HOME", dir.path());
    let source = FileToolSource::new(dir.path()).unwrap();
    let todos = json!([
        { "id": "1", "content": "First task", "status": "pending", "priority": "high" },
        { "id": "2", "content": "Second task", "status": "completed", "priority": "medium" }
    ]);
    let write_result = source
        .call_tool(TOOL_TODO_WRITE, json!({ "todos": todos }))
        .await
        .unwrap();
    assert!(write_result.text.contains("1 todos"));
    let read_result = source.call_tool(TOOL_TODO_READ, json!({})).await.unwrap();
    assert!(read_result.text.contains("1 todos"));
    assert!(read_result.text.contains("First task"));
    assert!(read_result.text.contains("Second task"));
    assert!(read_result.text.contains("completed"));
}

/// Scenario: todo_read when XDG todo file does not exist returns empty list (0 todos).
#[tokio::test]
async fn todo_read_when_file_missing_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_STATE_HOME", dir.path());
    let source = FileToolSource::new(dir.path()).unwrap();
    let result = source.call_tool(TOOL_TODO_READ, json!({})).await.unwrap();
    assert!(result.text.starts_with("0 todos"));
    assert!(result.text.contains("[]"));
}

/// Scenario: todo_write with missing 'todos' returns InvalidInput.
#[tokio::test]
async fn todo_write_missing_todos_returns_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_STATE_HOME", dir.path());
    let source = FileToolSource::new(dir.path()).unwrap();
    let result = source.call_tool(TOOL_TODO_WRITE, json!({})).await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}
