//! Simple echo agent example using the loom Graph API.
//!
//! Run: `cargo run -p loom-examples --example echo`

use async_trait::async_trait;
use loom_graph_core::Agent;
use loom_llm::{error::GraphError, message::Message};
use std::env;

#[derive(Debug, Clone, Default)]
struct EchoState {
    messages: Vec<Message>,
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
    type State = EchoState;
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
        Ok(EchoState { messages })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = env::args()
        .nth(1)
        .unwrap_or_else(|| "Hello, world!".to_string());

    let agent = EchoAgent::new();
    let state = agent.run(EchoState {
        messages: vec![Message::user(input)],
    }).await?;

    if let Some(Message::Assistant(payload)) = state.messages.last() {
        println!("{}", payload.content);
    }

    Ok(())
}