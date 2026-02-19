//! Integration tests for BashToolsSource: list_tools and call_tool.

mod init_logging;

use loom::tool_source::{BashToolsSource, ToolSource};
use serde_json::json;

#[tokio::test]
async fn bash_tools_source_lists_bash_tool() {
    let source = BashToolsSource::new().await;
    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "bash");
    assert!(tools[0].description.is_some());
}

#[tokio::test]
async fn bash_tools_source_call_bash_success() {
    let source = BashToolsSource::new().await;
    let args = json!({ "command": "echo ok" });
    let result = source.call_tool("bash", args).await.unwrap();
    assert!(!result.text.is_empty());
    assert!(result.text.trim().contains("ok"));
}

#[tokio::test]
async fn bash_tools_source_call_bash_missing_command() {
    let source = BashToolsSource::new().await;
    let args = json!({});
    let result = source.call_tool("bash", args).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn bash_tools_source_call_nonexistent_tool() {
    let source = BashToolsSource::new().await;
    let args = json!({ "command": "echo x" });
    let result = source.call_tool("nonexistent", args).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("not found"));
}
