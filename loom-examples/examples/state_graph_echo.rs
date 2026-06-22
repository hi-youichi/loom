//! StateGraph with an echo Agent node.
//!
//! Run: `cargo run -p loom-examples --example state_graph_echo`

use std::sync::Arc;

use async_trait::async_trait;
use loom_graph::{Agent, AgentNode, CompiledStateGraph, StateGraph, END, START};
use loom_llm::{error::GraphError, message::Message};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AgentState {
    pub messages: Vec<Message>,
}

struct EchoAgent;

impl EchoAgent {
    fn new() -> Self {
        EchoAgent
    }
}

#[async_trait]
impl Agent for EchoAgent {
    fn name(&self) -> &str {
        "echo"
    }
    type State = AgentState;
    async fn run(&self, state: Self::State) -> Result<Self::State, GraphError> {
        let mut messages = state.messages;
        let last = messages.last().and_then(|m| {
            if let Message::User(s) = m {
                Some(s.clone())
            } else {
                None
            }
        });
        if let Some(content) = last {
            messages.push(Message::assistant(content));
        }
        Ok(AgentState { messages })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Hello, world!".to_string());

    let mut graph = StateGraph::<AgentState>::new();
    graph
        .add_node("echo", Arc::new(AgentNode::new(EchoAgent::new())))
        .add_edge(START, "echo")
        .add_edge("echo", END);

    let compiled: CompiledStateGraph<AgentState> = graph.compile()?;

    let state = AgentState {
        messages: vec![Message::user(input)],
    };

    let result = compiled.invoke(state, None).await?;

    if let Some(Message::Assistant(payload)) = result.messages.last() {
        println!("{}", payload.content);
    }

    Ok(())
}