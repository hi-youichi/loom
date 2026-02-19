//! Example: StateGraph with SQLite-persistent checkpointer (SqliteSaver).
//!
//! Same flow as memory_checkpoint but state is stored in a SQLite file and survives process restarts.
//! Run twice with the same thread_id to see persistence: first run saves; second run can load via get_tuple.
//! Design: 16-memory-design.md §3.6.
//!
//! Run: `cargo run -p loom-examples --example memory_persistence -- "hello"`
//! Or:  `mkdir -p data && cargo run -p loom-examples --example memory_persistence -- "hi"`

use async_trait::async_trait;
use loom::{
    Agent, AgentError, Checkpointer, JsonSerializer, Message, RunnableConfig, SqliteSaver,
    StateGraph, END, START,
};
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

fn db_path() -> std::path::PathBuf {
    let p = Path::new("data").join("memory_persistence.db");
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    p
}

#[tokio::main]
async fn main() {
    let input = env::args().nth(1).unwrap_or_else(|| "hello".to_string());
    let path = db_path();

    let serializer = Arc::new(JsonSerializer);
    let checkpointer: Arc<dyn Checkpointer<AgentState>> =
        Arc::new(SqliteSaver::new(&path, serializer).expect("SqliteSaver::new"));

    let config = RunnableConfig {
        thread_id: Some("session-persist".into()),
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
        .expect("compile");

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
        println!("persisted to: {}", path.display());
        assert_eq!(cp.channel_values.messages.len(), state.messages.len());
    }
}
