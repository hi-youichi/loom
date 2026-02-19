//! Exa MCP integration test over HTTP: connect to https://mcp.exa.ai/mcp via
//! `McpToolSource::new_http`, list_tools, and call_tool (e.g. web_search_exa).
//!
//! Loads `EXA_API_KEY` from `.env` or environment. Run with:
//!
//! ```bash
//! cargo test -p loom exa_http -- --ignored
//! ```

mod init_logging;

use loom::tool_source::{McpToolSource, ToolSource};

const DEFAULT_EXA_URL: &str = "https://mcp.exa.ai/mcp";

/// Connects to Exa MCP over HTTP, lists tools, and verifies at least one Exa tool exists.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires EXA_API_KEY and network; run with: cargo test -p loom exa_http -- --ignored"]
async fn exa_http_list_tools() {
    dotenv::dotenv().ok();
    let key = std::env::var("EXA_API_KEY")
        .expect("EXA_API_KEY must be set in .env or env for exa_http tests");
    let url = std::env::var("MCP_EXA_URL").unwrap_or_else(|_| DEFAULT_EXA_URL.to_string());

    let source = McpToolSource::new_http(url, [("EXA_API_KEY", key.as_str())])
        .await
        .expect("McpToolSource::new_http");

    let tools = source.list_tools().await.expect("list_tools");
    assert!(!tools.is_empty(), "expected at least one tool from Exa MCP");

    let known = [
        "web_search_exa",
        "get_code_context_exa",
        "company_research_exa",
    ];
    let has_known = tools.iter().any(|t| known.contains(&t.name.as_str()));
    assert!(
        has_known,
        "expected one of {:?}; got names: {:?}",
        known,
        tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>()
    );
}

/// Connects to Exa MCP over HTTP and calls web_search_exa with a minimal query.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires EXA_API_KEY and network; run with: cargo test -p loom exa_http -- --ignored"]
async fn exa_http_call_web_search() {
    dotenv::dotenv().ok();
    let key = std::env::var("EXA_API_KEY")
        .expect("EXA_API_KEY must be set in .env or env for exa_http tests");
    let url = std::env::var("MCP_EXA_URL").unwrap_or_else(|_| DEFAULT_EXA_URL.to_string());

    let source = McpToolSource::new_http(url, [("EXA_API_KEY", key.as_str())])
        .await
        .expect("McpToolSource::new_http");

    let tools = source.list_tools().await.expect("list_tools");
    if !tools.iter().any(|t| t.name == "web_search_exa") {
        eprintln!("web_search_exa not in tool list; skipping call_tool");
        return;
    }

    let args = serde_json::json!({ "query": "Rust programming language" });
    let content = source
        .call_tool("web_search_exa", args)
        .await
        .expect("call_tool web_search_exa");
    assert!(!content.text.is_empty(), "expected non-empty tool result");
}
