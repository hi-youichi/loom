//! Explicit tests for Agent-as-Node blanket impl (traits.rs).
//!
//! Verifies: id() equals name(); run success maps to Ok((state, Next::Continue));
//! run error is propagated unchanged.

use loom::{Agent, Message, Next, Node};

use crate::common::{AgentState, EchoAgent, FailingAgent};

/// **Scenario**: When an Agent is used as Node<S>, id() equals name().
#[tokio::test]
async fn agent_as_node_id_is_name() {
    let agent = EchoAgent::new();
    assert_eq!(
        agent.id(),
        agent.name(),
        "Node::id() must equal Agent::name()"
    );
    assert_eq!(agent.id(), "echo");
}

/// **Scenario**: When Agent::run returns Ok(state), Node::run returns Ok((state, Next::Continue)).
#[tokio::test]
async fn agent_as_node_run_maps_to_continue() {
    let agent = EchoAgent::new();
    let state = AgentState {
        messages: vec![Message::user("hello")],
    };
    let result = Node::run(&agent, state).await;
    let (out_state, next) = result.expect("EchoAgent run should succeed");
    assert!(matches!(next, Next::Continue));
    assert_eq!(out_state.messages.len(), 2);
    assert!(matches!(out_state.messages.last(), Some(Message::Assistant(s)) if s == "hello"));
}

/// **Scenario**: When Agent::run returns Err, Node::run propagates the same error.
#[tokio::test]
async fn agent_as_node_run_propagates_error() {
    let agent = FailingAgent::new();
    let state = AgentState::default();
    let result = Node::run(&agent, state).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().to_lowercase().contains("execution failed"));
    assert!(err.to_string().contains("always fails"));
}
