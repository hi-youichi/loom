//! Example: StateGraph with SQLite checkpointer (persistent checkpointing).
//!
//! Uses SqliteSaver for persistent checkpoint storage across process restarts.
//! Run: `cargo run -p loom-examples --example memory_persistence -- "hello"`

use async_trait::async_trait;
use loom_graph_core::{Agent, AgentNode, StateGraph, END, START};
use loom_llm::{error::GraphError, message::Message};
use checkpoint::{Checkpointer, JsonSerializer, RunnableConfig};
use sqlite_store::SqliteSaver;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    let input = env::args().nth(1).unwrap_or_else(|| "hello".to_string());

    let path = env::var("LOOM_CHECKPOINT_DB")
        .unwrap_or_else(|_| "echo_checkpoints.db".to_string());

    let serializer = Arc::new(JsonSerializer);
    let checkpointer: Arc<dyn Checkpointer<AgentState>> =
        Arc::new(SqliteSaver::new(&path, serializer)?);

    let config = RunnableConfig {
        thread_id: Some("session-persist".into()),
        ..Default::default()
    };

    let mut graph = StateGraph::<AgentState>::new();
    graph
        .add_node("echo", Arc::new(AgentNode::new(EchoAgent)))
        .add_edge(START, "echo")
        .add_edge("echo", END);

    let compiled = graph.compile_with_checkpointer(checkpointer.clone())?;

    let mut state = AgentState::default();
    state.messages.push(Message::user(input.clone()));

    let state = compiled.invoke(state, Some(config.clone())).await?;

    if let Some(Message::Assistant(payload)) = state.messages.last() {
        println!("{}", payload.content);
    }

    let tuple = checkpointer.get_tuple(&config).await?;
    if let Some((cp, _)) = tuple {
        println!("checkpoint id: {}", cp.id);
        println!("persisted to: {}", Path::new(&path).display());
        assert_eq!(cp.channel_values.messages.len(), state.messages.len());
    }

    Ok(())
}