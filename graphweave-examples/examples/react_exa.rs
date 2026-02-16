//! ReAct agent with Exa MCP: web search via Exa's MCP server (think → act → observe).
//!
//! Connects to Exa's hosted MCP at `https://mcp.exa.ai/mcp` using `mcp-remote` as a
//! stdio→HTTP bridge so `McpToolSource` can talk to to remote server. Uses a real
//! LLM (ChatOpenAI) so that model can choose tools like `web_search_exa`, `get_code_context_exa`,
//! or `company_research_exa`.
//!
//! ## Prerequisites
//!
//! - Node.js/npx (for `mcp-remote`). Install: `npm install -g mcp-remote` or use `npx -y mcp-remote`.
//! - OpenAI API key for LLM: set `OPENAI_API_KEY` in `.env` or environment.
//!
//! ## Usage
//!
//! ```bash
//! cargo run -p graphweave-examples --example react_exa
//! cargo run -p graphweave-examples --example react_exa -- "Search the web for latest Rust 2024 news"
//! ```
//!
//! ## Environment
//!
//! - `OPENAI_API_KEY`: Required for ChatOpenAI (do NOT commit).
//! - `EXA_API_KEY`: Optional. Exa hosted endpoint may work without it (rate-limited).
//!   If you use npm Exa MCP server locally, set this and point `MCP_EXA_URL` to your server.
//! - `MCP_EXA_URL`: Optional. Default `https://mcp.exa.ai/mcp`. Use another URL if self-hosting.
//! - `MCP_REMOTE_CMD`: Optional. Default `npx`. Use full path if npx is not in PATH.
//! - `MCP_REMOTE_ARGS`: Optional. Default `-y mcp-remote`. URL is appended if not present.

use std::sync::Arc;

use graphweave::{
    ActNode, ChatOpenAI, CompiledStateGraph, McpToolSource, Message, ObserveNode, ReActState,
    StateGraph, ThinkNode, ToolSource, END, REACT_SYSTEM_PROMPT, START,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();

    let user_input = std::env::args().nth(1).unwrap_or_else(|| {
        "Search the web for the latest news about Rust programming language in 2024.".to_string()
    });

    let exa_url =
        std::env::var("MCP_EXA_URL").unwrap_or_else(|_| "https://mcp.exa.ai/mcp".to_string());
    let cmd = std::env::var("MCP_REMOTE_CMD").unwrap_or_else(|_| "npx".to_string());
    let args_str = std::env::var("MCP_REMOTE_ARGS").unwrap_or_else(|_| "-y mcp-remote".to_string());
    let mut args: Vec<String> = args_str.split_whitespace().map(String::from).collect();
    if !args
        .iter()
        .any(|a| a.as_str() == exa_url.as_str() || a.contains("mcp.exa.ai"))
    {
        args.push(exa_url.clone());
    }

    let tool_source = if let Ok(key) = std::env::var("EXA_API_KEY") {
        let mut env: Vec<(String, String)> = vec![("EXA_API_KEY".to_string(), key)];
        if let Ok(home) = std::env::var("HOME") {
            env.push(("HOME".to_string(), home));
        }
        McpToolSource::new_with_env(cmd, args, env, false)?
    } else {
        McpToolSource::new(cmd, args, false)?
    };

    let tools = tool_source.list_tools().await?;
    let llm = ChatOpenAI::new("gpt-4o-mini").with_tools(tools);
    let think = ThinkNode::new(Box::new(llm));
    let act = ActNode::new(Box::new(tool_source));
    let observe = ObserveNode::new();

    let mut graph = StateGraph::<ReActState>::new();
    graph
        .add_node("think", Arc::new(think))
        .add_node("act", Arc::new(act))
        .add_node("observe", Arc::new(observe))
        .add_edge(START, "think")
        .add_edge("think", "act")
        .add_edge("act", "observe")
        .add_edge("observe", END);

    let compiled: CompiledStateGraph<ReActState> = graph.compile()?;
    let state = ReActState {
        messages: vec![
            Message::system(REACT_SYSTEM_PROMPT),
            Message::user(user_input.clone()),
        ],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
    };

    println!("User: {}", user_input);
    println!("---");

    match compiled.invoke(state, None).await {
        Ok(s) => {
            for m in &s.messages {
                match m {
                    Message::System(x) => println!("[System] {}", x),
                    Message::User(x) => println!("[User] {}", x),
                    Message::Assistant(x) => println!("[Assistant] {}", x),
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!(
                "\nEnsure OPENAI_API_KEY is set and npx/mcp-remote can reach {}",
                exa_url
            );
            std::process::exit(1);
        }
    }

    Ok(())
}
