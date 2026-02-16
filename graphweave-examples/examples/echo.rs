//! Echo example: state-in, state-out (GraphWeave-style).
//!
//! EchoAgent and AgentState are implemented here as examples of using the minimal
//! Agent trait; they are not part of the framework.
//! Run: `cargo run -p graphweave-examples --example echo -- "Hello"`

use async_trait::async_trait;
use graphweave::{Agent, AgentError, Message};
use std::env;

/// Example state: message list only (defined in example, not in framework).
#[derive(Debug, Clone, Default)]
struct AgentState {
    pub messages: Vec<Message>,
}

/// Example agent: if the last message is `User(s)`, appends `Assistant(s)`.
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

    let mut state = AgentState::default();
    state.messages.push(Message::User(input));

    let agent = EchoAgent::new();
    match agent.run(state).await {
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
