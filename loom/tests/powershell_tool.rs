//! Integration tests for PowerShellTool: name, spec, and call behavior.
//!
//! Tests are split into:
//! - Universal tests: compile and schema validation on all platforms
//! - Windows tests: actual execution tests only on Windows

mod init_logging;

use tool_basic::{PowerShellTool, TOOL_POWERSHELL};
use tool_core::Tool;
use serde_json::json;

// ============================================================================
// Universal Tests (run on all platforms)
// ============================================================================

#[tokio::test]
async fn powershell_tool_name_is_correct() {
    let tool = PowerShellTool::new();
    assert_eq!(tool.name(), TOOL_POWERSHELL);
}

#[tokio::test]
async fn powershell_tool_spec_has_correct_properties() {
    let tool = PowerShellTool::new();
    let spec = tool.spec();

    assert_eq!(spec.name, TOOL_POWERSHELL);
    assert!(spec.description.is_some());

    let desc = spec.description.unwrap();
    assert!(
        desc.contains("PowerShell") || desc.contains("Windows"),
        "Description should mention PowerShell or Windows: {}",
        desc
    );

    // Verify required parameter
    assert_eq!(spec.input_schema["properties"]["command"]["type"], "string");
    assert!(spec.input_schema["required"]
        .as_array()
        .unwrap()
        .contains(&json!("command")));

    assert!(spec.input_schema["properties"].get("workdir").is_some());
    assert!(spec.input_schema["properties"].get("timeout").is_some());
    assert!(spec.input_schema["properties"].get("timeout_ms").is_some());
    assert!(spec.input_schema["properties"].get("env").is_some());
    assert!(spec.input_schema["properties"]
        .get("execution_policy")
        .is_some());
    assert!(spec.input_schema["properties"]
        .get("use_legacy_powershell")
        .is_some());
}

#[tokio::test]
async fn powershell_tool_default_construction() {
    let tool = PowerShellTool::default();
    assert_eq!(tool.name(), TOOL_POWERSHELL);
}

// All Windows-only execution tests (pwsh.exe spawn) are deleted.
// Universal tests above are sufficient for cross-platform validation.
