//! Unit tests for NameNode: no-op node that only has a name and passes state through.
//!
//! NameNode implements `Node<S>` for any state type; run returns the same state and
//! `Next::Continue`. These tests verify id, pass-through behaviour, and use in a chain.

mod init_logging;

use std::sync::Arc;

use async_trait::async_trait;
use graphweave::{Agent, AgentError, Message, NameNode, StateGraph, END, START};

#[derive(Debug, Clone, Default)]
struct AgentState {
    pub messages: Vec<Message>,
}

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
        if let Some(Message::User(s)) = messages.last() {
            messages.push(Message::Assistant(s.clone()));
        }
        Ok(AgentState { messages })
    }
}

/// Given a chain placeholder (NameNode) -> echo (EchoAgent), when we invoke with a user message,
/// then state passes through NameNode unchanged and EchoAgent runs, so the final state contains
/// the echoed assistant message.
#[tokio::test]
async fn name_node_passes_through_state_and_continues() {
    let mut graph = StateGraph::<AgentState>::new();
    graph
        .add_node("placeholder", Arc::new(NameNode::new("placeholder")))
        .add_node("echo", Arc::new(EchoAgent::new()))
        .add_edge(START, "placeholder")
        .add_edge("placeholder", "echo")
        .add_edge("echo", END);

    let compiled = graph.compile().unwrap();
    let mut state = AgentState::default();
    state.messages.push(Message::User("hi".into()));

    let state = compiled.invoke(state, None).await.unwrap();
    let last = state.messages.last().unwrap();
    assert!(matches!(last, Message::Assistant(s) if s == "hi"));
}
