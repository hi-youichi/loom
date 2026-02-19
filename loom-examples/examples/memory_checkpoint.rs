//! Example: StateGraph with checkpointer (MemorySaver).
//!
//! Builds a linear graph, compiles with MemorySaver, invokes with thread_id in config.
//! Final state is saved after invoke; get_tuple can load the last checkpoint. Design: 16-memory-design.md.
//!
//! Run: `cargo run -p loom-examples --example memory_checkpoint -- "hello"`

use async_trait::async_trait;
use loom::{
    Agent, AgentError, Checkpointer, MemorySaver, Message, RunnableConfig, StateGraph, END, START,
};
use std::env;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
struct AgentState {
    pub messages: Vec<Message>,
}

struct EchoAgent;

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
    let input = env::args().nth(1).unwrap_or_else(|| "hello".to_string());

    let checkpointer: Arc<MemorySaver<AgentState>> = Arc::new(MemorySaver::new());
    let config = RunnableConfig {
        thread_id: Some("session-1".into()),
        checkpoint_id: None,
        checkpoint_ns: String::new(),
        user_id: None,
        resume_from_node_id: None,
    };

    let mut graph = StateGraph::<AgentState>::new();
    graph
        .add_node("echo", Arc::new(EchoAgent))
        .add_edge(START, "echo")
        .add_edge("echo", END);

    let compiled = graph
        .compile_with_checkpointer(checkpointer.clone())
        .expect("valid graph");

    let mut state = AgentState::default();
    state.messages.push(Message::User(input.clone()));

    let state = compiled
        .invoke(state, Some(config.clone()))
        .await
        .expect("invoke");

    if let Some(Message::Assistant(content)) = state.messages.last() {
        println!("{content}");
    }

    let tuple = checkpointer.get_tuple(&config).await.expect("get_tuple");
    if let Some((cp, _)) = tuple {
        println!("checkpoint id: {}", cp.id);
        assert_eq!(cp.channel_values.messages.len(), state.messages.len());
    }
}
