//! StateGraph invoke: single-node chain produces correct output.

use std::sync::Arc;

use loom::{Message, StateGraph, END, START};

use crate::common::{AgentState, EchoAgent};

#[tokio::test]
async fn invoke_single_node_chain() {
    let mut graph = StateGraph::<AgentState>::new();
    graph
        .add_node("echo", Arc::new(EchoAgent::new()))
        .add_edge(START, "echo")
        .add_edge("echo", END);

    let compiled = graph.compile().unwrap();
    let mut state = AgentState::default();
    state.messages.push(Message::User("hi".into()));

    let state = compiled.invoke(state, None).await.unwrap();
    let last = state.messages.last().unwrap();
    assert!(matches!(last, Message::Assistant(s) if s == "hi"));
}
