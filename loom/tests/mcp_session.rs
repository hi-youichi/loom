//! McpSession integration test: spawn mcp-filesystem-server, list tools, call tool.
//!
//! Requires `mcp-filesystem-server` to be buildable. Run from workspace root:
//! `cargo test -p loom mcp_session -- --ignored` to run (ignored by default
//! as it spawns external process).

mod init_logging;

use loom::tool_source::McpSession;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns mcp-filesystem-server; run with --ignored"]
async fn mcp_session_list_and_call_tool() {
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

    let mut session = McpSession::new(command, args, None::<Vec<(String, String)>>, true)
        .expect("McpSession::new");

    session
        .send_request("test-tools-list", "tools/list", serde_json::json!({}))
        .expect("send_request tools/list");

    let result = session
        .wait_for_result("test-tools-list", std::time::Duration::from_secs(15))
        .expect("wait_for_result")
        .expect("got result");

    assert!(
        result.error.is_none(),
        "tools/list error: {:?}",
        result.error
    );
    let tools = result
        .result
        .as_ref()
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .expect("tools array");
    assert!(!tools.is_empty(), "expected at least one tool");

    let has_list_dir = tools.iter().any(|t| {
        t.get("name")
            .and_then(|n| n.as_str())
            .map(|n| n == "list_directory")
            .unwrap_or(false)
    });
    assert!(has_list_dir, "expected list_directory tool");

    let path = std::env::current_dir()
        .map(|p| format!("file://{}", p.display()))
        .unwrap_or_else(|_| "file:///tmp".to_string());
    session
        .send_request(
            "test-call",
            "tools/call",
            serde_json::json!({
                "name": "list_directory",
                "arguments": { "path": path }
            }),
        )
        .expect("send_request tools/call");

    let call_result = session
        .wait_for_result("test-call", std::time::Duration::from_secs(10))
        .expect("wait_for_result")
        .expect("got call result");

    assert!(
        call_result.error.is_none(),
        "tools/call error: {:?}",
        call_result.error
    );
    let content = call_result
        .result
        .as_ref()
        .and_then(|r| r.get("content"))
        .and_then(|c| c.as_array())
        .expect("content array");
    assert!(!content.is_empty(), "expected content");
}
