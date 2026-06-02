//! Explicit tests for AgentNode wrapper (traits.rs).
//!
//! Verifies: id() equals name(); run success maps to Ok((state, Next::Continue));
//! run error is propagated unchanged.

use loom::{AgentNode, Message, Next, Node};

use crate::common::{AgentState, EchoAgent, FailingAgent};

/// **Scenario**: When an Agent is wrapped in AgentNode, id() equals name().
#[tokio::test]
async fn agent_as_node_id_is_name() {
    let agent = AgentNode::new(EchoAgent::new());
    assert_eq!(
        Node::id(&agent),
        "echo",
        "Node::id() must equal the wrapped agent's name"
    );
}

/// **Scenario**: When Agent::run returns Ok(state), AgentNode::run returns Ok((state, Next::Continue)).
#[tokio::test]
async fn agent_as_node_run_maps_to_continue() {
    let agent = AgentNode::new(EchoAgent::new());
    let state = AgentState {
        messages: vec![Message::user("hello")],
    };
    let result = Node::run(&agent, state).await;
    let (out_state, next) = result.expect("EchoAgent run should succeed");
    assert!(matches!(next, Next::Continue));
    assert_eq!(out_state.messages.len(), 2);
    assert!(
        matches!(out_state.messages.last(), Some(Message::Assistant(p)) if p.content == "hello")
    );
}

/// **Scenario**: When Agent::run returns Err, AgentNode::run propagates the same error.
#[tokio::test]
async fn agent_as_node_run_propagates_error() {
    let agent = AgentNode::new(FailingAgent::new());
    let state = AgentState::default();
    let result = Node::run(&agent, state).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().to_lowercase().contains("execution failed"));
    assert!(err.to_string().contains("always fails"));
}
