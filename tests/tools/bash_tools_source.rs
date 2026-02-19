//! Unit tests for BashToolsSource: list_tools and call_tool for the bash tool.

use loom::tool_source::{BashToolsSource, ToolSource};
use serde_json::json;

/// **Scenario**: list_tools returns one tool named "bash".
#[tokio::test]
async fn bash_tools_source_lists_bash_tool() {
    let source = BashToolsSource::new().await;
    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "bash");
    assert!(tools[0].description.is_some());
    assert!(
        tools[0]
            .description
            .as_ref()
            .unwrap()
            .contains("shell") || tools[0].description.as_ref().unwrap().contains("command"),
        "{:?}",
        tools[0].description
    );
}

/// **Scenario**: call_tool("bash", { "command": "echo ok" }) returns output containing "ok".
#[tokio::test]
async fn bash_tools_source_call_bash_success() {
    let source = BashToolsSource::new().await;
    let args = json!({ "command": "echo ok" });
    let result = source.call_tool("bash", args).await.unwrap();
    assert!(!result.text.is_empty());
    assert!(result.text.trim().contains("ok"));
}

/// **Scenario**: call_tool("bash", {}) returns error (missing command).
#[tokio::test]
async fn bash_tools_source_call_bash_missing_command() {
    let source = BashToolsSource::new().await;
    let args = json!({});
    let result = source.call_tool("bash", args).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("missing") || err.to_string().contains("invalid"),
        "{}",
        err
    );
}

/// **Scenario**: call_tool with nonexistent tool name returns NotFound.
#[tokio::test]
async fn bash_tools_source_call_nonexistent_tool() {
    let source = BashToolsSource::new().await;
    let args = json!({ "command": "echo x" });
    let result = source.call_tool("nonexistent", args).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().to_lowercase().contains("not found"));
}

/// **Scenario**: call_tool_with_context("bash", args, None) runs the command.
#[tokio::test]
async fn bash_tools_source_call_tool_with_context() {
    let source = BashToolsSource::new().await;
    let args = json!({ "command": "printf hello" });
    let result = source
        .call_tool_with_context("bash", args, None)
        .await
        .unwrap();
    assert!(result.text.contains("hello"));
}
