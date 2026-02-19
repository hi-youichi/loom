//! McpToolSource integration test: connect to mcp-filesystem-server, list_tools, call_tool.
//!
//! Run with: `cargo test -p loom mcp_tool_source -- --ignored`

mod init_logging;

use loom::tool_source::{McpToolSource, ToolSource};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns mcp-filesystem-server; run with --ignored"]
async fn mcp_tool_source_list_and_call() {
    let command = std::env::var("MCP_SERVER_COMMAND").unwrap_or_else(|_| "cargo".to_string());
    let args = std::env::var("MCP_SERVER_ARGS")
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_else(|_| {
            vec![
                "run".into(),
                "-p".into(),
                "mcp-filesystem-server".into(),
                "--quiet".into(),
            ]
        });

    let source = McpToolSource::new(command, args, true).expect("McpToolSource::new");
    let tools = source.list_tools().await.expect("list_tools");
    assert!(!tools.is_empty(), "expected at least one tool");

    let list_dir = tools
        .iter()
        .find(|t| t.name == "list_directory")
        .expect("list_directory tool");
    assert!(list_dir.description.is_some() || list_dir.input_schema.is_object());

    let path = std::env::current_dir()
        .map(|p| format!("file://{}", p.display()))
        .unwrap_or_else(|_| "file:///tmp".to_string());
    let content = source
        .call_tool("list_directory", serde_json::json!({ "path": path }))
        .await
        .expect("call_tool");
    assert!(!content.text.is_empty());
}
