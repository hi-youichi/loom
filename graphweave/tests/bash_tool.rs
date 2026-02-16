//! Integration tests for BashTool: name, spec, and call behavior.

mod init_logging;

use graphweave::tools::{BashTool, Tool, TOOL_BASH};
use serde_json::json;

#[tokio::test]
async fn bash_tool_name_returns_bash() {
    let tool = BashTool::new();
    assert_eq!(tool.name(), TOOL_BASH);
}

#[tokio::test]
async fn bash_tool_spec_has_correct_properties() {
    let tool = BashTool::new();
    let spec = tool.spec();
    assert_eq!(spec.name, TOOL_BASH);
    assert!(spec.description.is_some());
    let desc = spec.description.unwrap();
    assert!(desc.contains("shell") || desc.contains("command"));
    assert_eq!(spec.input_schema["properties"]["command"]["type"], "string");
    assert!(spec.input_schema["required"]
        .as_array()
        .unwrap()
        .contains(&json!("command")));
}

#[tokio::test]
async fn bash_tool_call_echo_returns_hello() {
    let tool = BashTool::new();
    let args = json!({ "command": "echo hello" });
    let result = tool.call(args, None).await.unwrap();
    assert!(result.text.trim().contains("hello"));
}

#[tokio::test]
async fn bash_tool_call_missing_command_returns_error() {
    let tool = BashTool::new();
    let args = json!({});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("missing") || err.to_string().contains("InvalidInput"),
        "{}",
        err
    );
}

#[tokio::test]
async fn bash_tool_default_construction() {
    let tool = BashTool::default();
    assert_eq!(tool.name(), TOOL_BASH);
}
