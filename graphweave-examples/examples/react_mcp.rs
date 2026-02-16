//! ReAct with McpToolSource: connects to MCP server (e.g. mcp-filesystem-server), runs think → act → observe.
//!
//!
//! ## Prerequisites
//!
//! Build the MCP filesystem server:
//!   `cargo build -p mcp-filesystem-server`
//!
//! ## Usage
//!
//! ```bash
//! cargo run -p graphweave-examples --example react_mcp
//! cargo run -p graphweave-examples --example react_mcp -- "List files in current directory"
//! ```
//!
//! ## Environment
//!
//! - `MCP_SERVER_COMMAND`: default `cargo`
//! - `MCP_SERVER_ARGS`: default `run -p mcp-filesystem-server --quiet`
//!
//! To use a different MCP server, set both accordingly.

use std::sync::Arc;

use graphweave::{
    ActNode, CompiledStateGraph, McpToolSource, Message, MockLlm, ObserveNode, ReActState,
    StateGraph, ThinkNode, ToolCall, END, START,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "List files in the current directory.".to_string());

    let path = std::env::current_dir()
        .map(|p| format!("file://{}", p.display()))
        .unwrap_or_else(|_| "file:///tmp".to_string());

    let tool_source = {
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
        McpToolSource::new(command, args, false)?
    };

    let mock_llm = MockLlm::new(
        "I'll list the directory for you.",
        vec![ToolCall {
            name: "list_directory".to_string(),
            arguments: serde_json::json!({ "path": path }).to_string(),
            id: Some("call-1".to_string()),
        }],
    );

    let mut graph = StateGraph::<ReActState>::new();
    graph
        .add_node("think", Arc::new(ThinkNode::new(Box::new(mock_llm))))
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
