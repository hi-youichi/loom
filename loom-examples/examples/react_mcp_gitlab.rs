//! ReAct with GitLab MCP: connects to GitLab MCP server, runs think → act → observe.
//!
//! Uses `McpToolSource::new_with_env` to pass GITLAB_TOKEN (and optional GITLAB_URL)
//! to the MCP server process. MockLlm calls `list_projects` with `per_page: 5`.
//!
//! ## Prerequisites
//!
//! - GitLab MCP server built (e.g. from `/Users/apple/dev/gitlab-mcp` or workspace
//!   `mcp-impls/gitlab-mcp`: `cargo build -p gitlab-mcp-server --release`)
//!
//! ## Usage
//!
//! ```bash
//! export GITLAB_TOKEN="glpat-xxx"
//! export MCP_SERVER_COMMAND="/path/to/gitlab-mcp-server"
//! cargo run -p loom-examples --example react_mcp_gitlab
//! ```
//!
//! ## Environment
//!
//! - `GITLAB_TOKEN`: **Required**. GitLab personal access token (do NOT commit).
//! - `GITLAB_URL`: Optional. Default `https://gitlab.com`.
//! - `MCP_SERVER_COMMAND`: Path to gitlab-mcp-server binary. Default `gitlab-mcp-server` (PATH).
//! - `MCP_SERVER_ARGS`: Optional args. Default empty.
//!
//! Use `.env` in workspace root (gitignored) or export before running.

use std::sync::Arc;

use loom::{
    ActNode, CompiledStateGraph, McpToolSource, Message, MockLlm, ObserveNode, ReActState,
    StateGraph, ThinkNode, ToolCall, END, START,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "List my GitLab projects.".to_string());

    let token = std::env::var("GITLAB_TOKEN")
        .map_err(|_| "GITLAB_TOKEN is required. Set it via environment or .env (do NOT commit).")?;

    let mut env: Vec<(String, String)> = vec![("GITLAB_TOKEN".to_string(), token)];
    if let Ok(url) = std::env::var("GITLAB_URL") {
        env.push(("GITLAB_URL".to_string(), url));
    }
    if let Ok(home) = std::env::var("HOME") {
        env.push(("HOME".to_string(), home));
    }

    let command =
        std::env::var("MCP_SERVER_COMMAND").unwrap_or_else(|_| "gitlab-mcp-server".to_string());
    let args = std::env::var("MCP_SERVER_ARGS")
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_default();

    let tool_source = McpToolSource::new_with_env(command, args, env, false)?;

    let mock_llm = MockLlm::new(
        "I'll list your GitLab projects.",
        vec![ToolCall {
            name: "list_projects".to_string(),
            arguments: serde_json::json!({ "per_page": 5 }).to_string(),
            id: Some("call-1".to_string()),
        }],
    );

    let mut graph = StateGraph::<ReActState>::new();
    graph
        .add_node("think", Arc::new(ThinkNode::new(Arc::new(mock_llm))))
        .add_node("act", Arc::new(ActNode::new(Box::new(tool_source))))
        .add_node("observe", Arc::new(ObserveNode::new()))
        .add_edge(START, "think")
        .add_edge("think", "act")
        .add_edge("act", "observe")
        .add_edge("observe", END);

    let compiled: CompiledStateGraph<ReActState> = graph.compile()?;

    let state = ReActState {
        messages: vec![Message::user(input)],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };

    let result = compiled.invoke(state, None).await?;
    for m in &result.messages {
        match m {
            Message::System(x) => println!("[System] {}", x),
            Message::User(x) => println!("[User] {}", x),
            Message::Assistant(x) => println!("[Assistant] {}", x),
        }
    }
    Ok(())
}
