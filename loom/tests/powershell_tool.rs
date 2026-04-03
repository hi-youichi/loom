//! Integration tests for PowerShellTool: name, spec, and call behavior.
//!
//! Tests are split into:
//! - Universal tests: compile and schema validation on all platforms
//! - Windows tests: actual execution tests only on Windows

mod init_logging;

use loom::tools::{PowerShellTool, Tool, TOOL_POWERSHELL};
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

// ============================================================================
// Windows-only Tests (actual execution)
// ============================================================================

#[cfg(windows)]
mod windows_tests {
    use super::*;

    #[tokio::test]
    async fn call_get_location_returns_path() {
        let tool = PowerShellTool::new();
        let args = json!({ "command": "Get-Location" });
        let result = tool.call(args, None).await.unwrap();

        // Should contain a path separator (Windows uses backslash)
        assert!(
            result.as_text().unwrap().contains('\\') || result.as_text().unwrap().contains(':'),
            "Get-Location should return a Windows path: {}",
            result.as_text().unwrap()
        );
    }

    #[tokio::test]
    async fn call_echo_returns_hello() {
        let tool = PowerShellTool::new();
        let args = json!({ "command": "Write-Output 'hello from ps'" });
        let result = tool.call(args, None).await.unwrap();
        assert!(result.as_text().unwrap().contains("hello from ps"));
    }

    #[tokio::test]
    async fn call_with_workdir_changes_directory() {
        let tool = PowerShellTool::new();
        let args = json!({
            "command": "Get-Location",
            "workdir": "C:\\Windows"
        });
        let result = tool.call(args, None).await.unwrap();
        assert!(result.as_text().unwrap().contains("Windows"));
    }

    #[tokio::test]
    async fn call_with_env_vars_exports_them() {
        let tool = PowerShellTool::new();
        let args = json!({
            "command": "$env:TEST_VAR",
            "env": {
                "TEST_VAR": "test_value_123"
            }
        });
        let result = tool.call(args, None).await.unwrap();
        assert!(result.as_text().unwrap().contains("test_value_123"));
    }

    #[tokio::test]
    async fn call_with_execution_policy_bypass() {
        let tool = PowerShellTool::new();
        let args = json!({
            "command": "Get-ExecutionPolicy",
            "execution_policy": "Bypass"
        });
        let result = tool.call(args, None).await.unwrap();
        // Should succeed without error
        assert!(!result.as_text().unwrap().to_lowercase().contains("error"));
    }

    #[tokio::test]
    async fn call_wmi_query_succeeds() {
        let tool = PowerShellTool::new();
        let args = json!({
            "command": "Get-WmiObject -Class Win32_ComputerSystem | Select-Object -First 1 | Format-List"
        });
        let result = tool.call(args, None).await.unwrap();
        // WMI query should return system info
        assert!(
            result.as_text().unwrap().contains(":") || result.as_text().unwrap().contains("Name"),
            "WMI query should return structured data: {}",
            &result.as_text().unwrap()[..100.min(result.as_text().unwrap().len())]
        );
    }

    #[tokio::test]
    async fn call_registry_read_succeeds() {
        let tool = PowerShellTool::new();
        let args = json!({
            "command": "Get-ItemProperty -Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion' -Name ProductName"
        });
        let result = tool.call(args, None).await.unwrap();
        // Should contain Windows version info
        assert!(
            result.as_text().unwrap().contains("Windows"),
            "Registry read should return Windows version: {}",
            &result.as_text().unwrap()[..200.min(result.as_text().unwrap().len())]
        );
    }

    #[tokio::test]
    async fn call_pipeline_succeeds() {
        let tool = PowerShellTool::new();
        let args = json!({
            "command": "Get-Process | Sort-Object CPU -Descending | Select-Object -First 3 | Format-Table Name, CPU"
        });
        let result = tool.call(args, None).await.unwrap();
        // Should return process table
        assert!(
            result.as_text().unwrap().contains("Name") || result.as_text().unwrap().contains("CPU"),
            "Pipeline should return process list: {}",
            &result.as_text().unwrap()[..100.min(result.as_text().unwrap().len())]
        );
    }

    #[tokio::test]
    async fn call_multiline_script_succeeds() {
        let tool = PowerShellTool::new();
        let args = json!({
            "command": "$a = 'hello'; $b = 'world'; Write-Output \"$a $b\""
        });
        let result = tool.call(args, None).await.unwrap();
        assert!(result.as_text().unwrap().contains("hello world"));
    }

    #[tokio::test]
    async fn call_invalid_command_reports_failure_in_output() {
        let tool = PowerShellTool::new();
        let args = json!({
            "command": "Get-InvalidCmdletThatDoesNotExist123"
        });
        let result = tool.call(args, None).await.expect("tool returns text");
        let lower = result.as_text().unwrap().to_lowercase();
        assert!(
            result.as_text().unwrap().contains("exited with code")
                || lower.contains("not recognized")
                || lower.contains("not found"),
            "expected cmdlet failure in output: {}",
            result.as_text().unwrap()
        );
    }

    #[tokio::test]
    async fn call_missing_command_returns_error() {
        let tool = PowerShellTool::new();
        let args = json!({});
        let result = tool.call(args, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("missing")
                || err.to_string().contains("command")
                || err.to_string().contains("InvalidInput"),
            "Error should mention missing command: {}",
            err
        );
    }

    #[tokio::test]
    async fn call_with_timeout_terminates_long_running() {
        let tool = PowerShellTool::new();
        let args = json!({
            "command": "Start-Sleep -Seconds 10",
            "timeout_ms": 500  // 500ms timeout
        });
        let result = tool.call(args, None).await;

        // Should timeout and return error
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("timeout") || msg.contains("timed out") || msg.contains("cancel"),
            "Should timeout: {}",
            err
        );
    }

    #[tokio::test]
    async fn stderr_is_captured() {
        let tool = PowerShellTool::new();
        let args = json!({
            "command": "[Console]::Error.WriteLine('stderr_line_test')"
        });
        let result = tool.call(args, None).await.expect("ok");
        assert!(
            result.as_text().unwrap().contains("stderr_line_test"),
            "stderr should appear in combined output: {}",
            result.as_text().unwrap()
        );
    }

    #[tokio::test]
    async fn working_folder_with_special_chars() {
        let tool = PowerShellTool::new();
        // Test with a path that has spaces (common on Windows)
        let args = json!({
            "command": "Get-Location",
            "workdir": "C:\\Program Files"
        });
        let result = tool.call(args, None).await;

        // Should handle spaces in path
        if let Ok(output) = result {
            assert!(output.as_text().unwrap().contains("Program Files"));
        }
    }
}
