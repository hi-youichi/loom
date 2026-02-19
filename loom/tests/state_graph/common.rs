//! Shared types for StateGraph integration tests: AgentState, EchoAgent.
//!
//! Used by compile_fail, invoke, store, and middleware test modules.

use async_trait::async_trait;
use loom::{Agent, AgentError, Message};

#[derive(Debug, Clone, Default)]
pub struct AgentState {
    pub messages: Vec<Message>,
}

pub struct EchoAgent;

impl EchoAgent {
    pub fn new() -> Self {
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
        if let Some(Message::User(s)) = messages.last() {
            messages.push(Message::Assistant(s.clone()));
        }
        Ok(AgentState { messages })
    }
}

/// Agent that always returns Err. Used to test Node::run error propagation.
pub struct FailingAgent;

impl FailingAgent {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Agent for FailingAgent {
    fn name(&self) -> &str {
        "failing"
    }
    type State = AgentState;
    async fn run(&self, _state: Self::State) -> Result<Self::State, AgentError> {
        Err(AgentError::ExecutionFailed("always fails".into()))
    }
}
