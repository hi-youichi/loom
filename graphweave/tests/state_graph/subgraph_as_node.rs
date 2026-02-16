//! CompiledStateGraph: Graph A runs two nodes then runs Graph B as a single node.
//!
//! Scenario: Graph A has node_a1 → node_a2 → subgraph_b → END. The node "subgraph_b"
//! is a wrapper that holds a compiled Graph B and invokes it with the current state.
//! This tests that one compiled graph can contain another graph as a node (subgraph-as-node).

use std::sync::Arc;

use async_trait::async_trait;
use graphweave::{
    AgentError, CompilationError, CompiledStateGraph, Next, Node, StateGraph, END, START,
};

// --- Shared state and nodes for Graph A and Graph B ---

/// Node that adds a constant to state (used in Graph A).
struct AddNode {
    id: &'static str,
    delta: i32,
}

#[async_trait]
impl Node<i32> for AddNode {
    fn id(&self) -> &str {
        self.id
    }
    async fn run(&self, state: i32) -> Result<(i32, Next), AgentError> {
        Ok((state + self.delta, Next::Continue))
    }
}

/// Node that multiplies state by a constant (used inside Graph B).
struct MulNode {
    id: &'static str,
    factor: i32,
}

#[async_trait]
impl Node<i32> for MulNode {
    fn id(&self) -> &str {
        self.id
    }
    async fn run(&self, state: i32) -> Result<(i32, Next), AgentError> {
        Ok((state * self.factor, Next::Continue))
    }
}

/// Wrapper node that runs a compiled subgraph (Graph B) with the current state.
/// Used to connect Graph A's next node to Graph B: when this node runs, it
/// invokes the inner CompiledStateGraph and returns the resulting state.
struct SubgraphNode {
    id: &'static str,
    inner: CompiledStateGraph<i32>,
}

#[async_trait]
impl Node<i32> for SubgraphNode {
    fn id(&self) -> &str {
        self.id
    }
    async fn run(&self, state: i32) -> Result<(i32, Next), AgentError> {
        let new_state = self.inner.invoke(state, None).await?;
        Ok((new_state, Next::Continue))
    }
}

/// Builds Graph B: a single node that multiplies state by 10.
fn build_graph_b() -> Result<CompiledStateGraph<i32>, CompilationError> {
    let mut graph = StateGraph::<i32>::new();
    graph
        .add_node("mul10", Arc::new(MulNode { id: "mul10", factor: 10 }))
        .add_edge(START, "mul10")
        .add_edge("mul10", END);
    graph.compile()
}

/// **Scenario**: Graph A runs node_a1, then node_a2, then executes Graph B as a single node.
/// Graph A's next node (after node_a2) is connected to Graph B via SubgraphNode.
/// State flow: 0 → a1(+1)=1 → a2(+2)=3 → graph_b(*10)=30.
#[tokio::test]
async fn graph_a_then_graph_b_as_node_produces_expected_state() {
    let compiled_b = build_graph_b().expect("graph B compiles");

    let mut graph_a = StateGraph::<i32>::new();
    graph_a
        .add_node("a1", Arc::new(AddNode { id: "a1", delta: 1 }))
        .add_node("a2", Arc::new(AddNode { id: "a2", delta: 2 }))
        .add_node(
            "subgraph_b",
            Arc::new(SubgraphNode {
                id: "subgraph_b",
                inner: compiled_b,
            }),
        )
        .add_edge(START, "a1")
        .add_edge("a1", "a2")
        .add_edge("a2", "subgraph_b")
        .add_edge("subgraph_b", END);

    let compiled_a = graph_a.compile().expect("graph A compiles");

    let initial: i32 = 0;
    let final_state = compiled_a.invoke(initial, None).await.unwrap();

    assert_eq!(final_state, 30, "0 -> a1(1) -> a2(3) -> graph_b(30)");
}
