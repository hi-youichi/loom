//! Unit tests for LspTool: name, spec, and call behavior.

use loom::tools::{LspTool, Tool, TOOL_LSP};
use serde_json::json;

/// **Scenario**: Tool name is "lsp".
#[tokio::test]
async fn lsp_tool_name_returns_lsp() {
    let tool = LspTool::placeholder();
    assert_eq!(tool.name(), TOOL_LSP);
}

/// **Scenario**: Spec has correct name, description, and required "action" in schema.
#[tokio::test]
async fn lsp_tool_spec_has_correct_properties() {
    let tool = LspTool::placeholder();
    let spec = tool.spec();
    assert_eq!(spec.name, TOOL_LSP);
    assert!(spec.description.is_some());
    
    // Verify input schema has required properties
    let schema = &spec.input_schema;
    let properties = schema.get("properties").unwrap();
    assert!(properties.get("action").is_some());
    assert!(properties.get("file_path").is_some());
}

/// **Scenario**: Spec has all required LSP actions.
#[tokio::test]
async fn lsp_tool_spec_has_all_actions() {
    let tool = LspTool::placeholder();
    let spec = tool.spec();
    let schema = &spec.input_schema;
    let properties = schema.get("properties").unwrap();
    let action = properties.get("action").unwrap();
    let enum_values = action.get("enum").unwrap().as_array().unwrap();
    
    let actions: Vec<&str> = enum_values.iter()
        .filter_map(|v| v.as_str())
        .collect();
    
    assert!(actions.contains(&"completion"));
    assert!(actions.contains(&"diagnostics"));
    assert!(actions.contains(&"gotoDefinition"));
    assert!(actions.contains(&"findReferences"));
    assert!(actions.contains(&"hover"));
    assert!(actions.contains(&"documentSymbols"));
}

/// **Scenario**: Default construction yields a tool with name "lsp".
#[tokio::test]
async fn lsp_tool_default_construction() {
    let tool = LspTool::default();
    assert_eq!(tool.name(), TOOL_LSP);
}

/// **Scenario**: Call with invalid action returns error.
#[tokio::test]
async fn lsp_tool_call_invalid_action_returns_error() {
    let tool = LspTool::placeholder();
    let args = json!({
        "action": "invalid_action",
        "file_path": "test.rs"
    });
    let result = tool.call(args, None).await;
    assert!(result.is_err());
}

/// **Scenario**: Call with missing file_path returns error.
#[tokio::test]
async fn lsp_tool_call_missing_file_path_returns_error() {
    let tool = LspTool::placeholder();
    let args = json!({
        "action": "diagnostics"
    });
    let result = tool.call(args, None).await;
    assert!(result.is_err());
}
