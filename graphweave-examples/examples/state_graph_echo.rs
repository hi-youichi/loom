//! StateGraph example: linear chain with EchoAgent.
//!
//! Single-node chain START → echo → END. Same behavior as the echo example
//! but via StateGraph. Run: `cargo run -p graphweave-examples --example state_graph_echo -- "Hello"`

use std::env;
use std::sync::Arc;

use async_trait::async_trait;
use graphweave::{Agent, AgentError, CompiledStateGraph, Message, StateGraph, END, START};

/// Example state: message list only (same as echo example).
#[derive(Debug, Clone, Default)]
struct AgentState {
    pub messages: Vec<Message>,
}

/// Example agent: if the last message is User(s), appends Assistant(s).
struct EchoAgent;

impl EchoAgent {
    fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Agent for EchoAgent {
    fn name(&self) -> &str {
        "echo"
    }

    type State = AgentState;

    async fn run(&self, state: Self::State) -> Result<Self::State, AgentError> {
        let mut messages = state.messages;
        let last = messages.last().and_then(|m| {
            if let Message::User(s) = m {
                Some(s.clone())
            } else {
                None
            }
        });
        if let Some(content) = last {
            messages.push(Message::Assistant(content));
        }
        Ok(AgentState { messages })
    }
}

#[tokio::main]
async fn main() {
    let input = env::args()
        .nth(1)
        .unwrap_or_else(|| "hello world".to_string());

    let mut graph = StateGraph::<AgentState>::new();
    graph
        .add_node("echo", Arc::new(EchoAgent::new()))
        .add_edge(START, "echo")
        .add_edge("echo", END);

    let compiled: CompiledStateGraph<AgentState> = graph.compile().expect("valid graph");

    let mut state = AgentState::default();
    state.messages.push(Message::User(input));

    match compiled.invoke(state, None).await {
        Ok(s) => {
            if let Some(Message::Assistant(content)) = s.messages.last() {
                println!("{content}");
            } else {
                eprintln!("no assistant reply");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
