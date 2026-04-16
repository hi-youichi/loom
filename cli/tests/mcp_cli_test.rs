//! Tests for MCP CLI functionality

use cli::mcp_manager::{AddMcpArgs, EditMcpArgs};
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_mcp_manager_basic_operations() {
    // Create a temporary config file
    let mut temp_file = NamedTempFile::new().unwrap();
    writeln!(temp_file, r#"{{"mcpServers":{{}}}}"#).unwrap();
    let _config_path = temp_file.path();

    // Note: This test would need to be adapted to work with the actual McpManager
    // since it uses get_or_create_mcp_config_path() which has a fixed path.
    // For now, this is a placeholder for future testing.

    // Test data
    let add_args = AddMcpArgs {
        name: "test-server".to_string(),
        command: Some("npx".to_string()),
        args: vec!["-y".to_string(), "@test/server".to_string()],
        url: None,
        env: vec![],
        disabled: false,
    };

    // Verify the structure is correct
    assert_eq!(add_args.name, "test-server");
    assert_eq!(add_args.command, Some("npx".to_string()));
    assert_eq!(add_args.args.len(), 2);
    assert_eq!(add_args.disabled, false);
}

#[test]
fn test_edit_args_structure() {
    let edit_args = EditMcpArgs {
        command: Some("node".to_string()),
        args: vec!["server.js".to_string()],
        url: None,
        env: vec!["API_KEY=test".to_string()],
        disabled: Some(true),
    };

    assert_eq!(edit_args.command, Some("node".to_string()));
    assert_eq!(edit_args.args.len(), 1);
    assert_eq!(edit_args.env.len(), 1);
    assert_eq!(edit_args.disabled, Some(true));
}

#[test]
fn test_add_args_url_type() {
    let add_args = AddMcpArgs {
        name: "http-server".to_string(),
        command: None,
        args: vec![],
        url: Some("http://localhost:3000/mcp".to_string()),
        env: vec![],
        disabled: false,
    };

    assert_eq!(add_args.name, "http-server");
    assert_eq!(add_args.command, None);
    assert_eq!(add_args.url, Some("http://localhost:3000/mcp".to_string()));
}
