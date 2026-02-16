//! StateGraph compile failure cases: unknown node, invalid chain, etc.

use std::sync::Arc;

use graphweave::{CompilationError, StateGraph, END, START};

use crate::common::{AgentState, EchoAgent};

/// **Scenario**: When an edge references a node not registered via add_node, compile returns NodeNotFound.
#[tokio::test]
async fn compile_fails_when_edge_refers_to_unknown_node() {
    let mut graph = StateGraph::<AgentState>::new();
    graph.add_node("echo", Arc::new(EchoAgent::new()));
    graph.add_edge(START, "echo");
    graph.add_edge("echo", "missing");

    match graph.compile() {
        Err(CompilationError::NodeNotFound(id)) => assert_eq!(id, "missing"),
        _ => panic!("expected NodeNotFound"),
    }
}

/// **Scenario**: When no edge has from_id == START, compile returns MissingStart.
#[tokio::test]
async fn compile_fails_when_no_edge_from_start() {
    let mut graph = StateGraph::<AgentState>::new();
    graph.add_node("echo", Arc::new(EchoAgent::new()));
    graph.add_edge("echo", END);

    match graph.compile() {
        Err(CompilationError::MissingStart) => {}
        Ok(_) => panic!("expected MissingStart"),
        Err(e) => panic!("expected MissingStart, got {:?}", e),
    }
}

/// **Scenario**: When no edge has to_id == END, compile returns MissingEnd.
#[tokio::test]
async fn compile_fails_when_no_edge_to_end() {
    let mut graph = StateGraph::<AgentState>::new();
    graph.add_node("echo", Arc::new(EchoAgent::new()));
    graph.add_edge(START, "echo");

    match graph.compile() {
        Err(CompilationError::MissingEnd) => {}
        Ok(_) => panic!("expected MissingEnd"),
        Err(e) => panic!("expected MissingEnd, got {:?}", e),
    }
}

/// **Scenario**: When more than one edge leaves START (branch), compile returns InvalidChain.
#[tokio::test]
async fn compile_fails_when_branch_from_start() {
    let mut graph = StateGraph::<AgentState>::new();
    graph.add_node("a", Arc::new(EchoAgent::new()));
    graph.add_node("b", Arc::new(EchoAgent::new()));
    graph.add_edge(START, "a");
    graph.add_edge(START, "b");
    graph.add_edge("a", END);
    graph.add_edge("b", END);

    match graph.compile() {
        Err(CompilationError::InvalidChain(_)) => {}
        Ok(_) => panic!("expected InvalidChain"),
        Err(e) => panic!("expected InvalidChain, got {:?}", e),
    }
}
