//! Unit tests for BashTool: name, spec, and call behavior.

use graphweave::tools::{BashTool, TOOL_BASH};
use serde_json::json;

/// **Scenario**: Tool name is "bash".
#[tokio::test]
async fn bash_tool_name_returns_bash() {
    let tool = BashTool::new();
    assert_eq!(tool.name(), TOOL_BASH);
}

/// **Scenario**: Spec has correct name, description, and required "command" in schema.
#[tokio::test]
async fn bash_tool_spec_has_correct_properties() {
    let tool = BashTool::new();
    let spec = tool.spec();
    assert_eq!(spec.name, TOOL_BASH);
    assert!(spec.description.is_some());
    assert!(spec.description.unwrap().contains("shell") || spec.description.unwrap().contains("command"));
    assert_eq!(spec.input_schema["properties"]["command"]["type"], "string");
    assert!(spec.input_schema["required"]
        .as_array()
        .unwrap()
        .contains(&json!("command")));
}

/// **Scenario**: Call with "echo hello" returns output containing "hello".
#[tokio::test]
async fn bash_tool_call_echo_returns_hello() {
    let tool = BashTool::new();
    let args = json!({ "command": "echo hello" });
    let result = tool.call(args, None).await.unwrap();
    assert!(result.text.trim().contains("hello"));
}

/// **Scenario**: Call with missing "command" returns InvalidInput error.
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

/// **Scenario**: Call with non-string "command" returns error.
#[tokio::test]
async fn bash_tool_call_non_string_command_returns_error() {
    let tool = BashTool::new();
    let args = json!({ "command": 42 });
    let result = tool.call(args, None).await;
    assert!(result.is_err());
}

/// **Scenario**: Default construction yields a tool with name "bash".
#[tokio::test]
async fn bash_tool_default_construction() {
    let tool = BashTool::default();
    assert_eq!(tool.name(), TOOL_BASH);
}
