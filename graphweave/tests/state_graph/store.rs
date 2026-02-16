//! StateGraph with_store: compiled graph holds store; store() returns Some (P5.2).

use std::sync::Arc;

use graphweave::{InMemoryStore, StateGraph, Store, END, START};

use crate::common::{AgentState, EchoAgent};

/// Compiled graph without `with_store` has no store (P5.2: do not break existing usage).
#[tokio::test]
async fn compile_without_store_has_no_store() {
    let mut graph = StateGraph::<AgentState>::new();
    graph
        .add_node("echo", Arc::new(EchoAgent::new()))
        .add_edge(START, "echo")
        .add_edge("echo", END);

    let compiled = graph.compile().unwrap();
    assert!(compiled.store().is_none());
}

/// Compiled graph with `with_store(store)` holds the store; `store()` returns Some (P5.2).
#[tokio::test]
async fn compile_with_store_holds_store() {
    let store: Arc<dyn Store> = Arc::new(InMemoryStore::new());
    let mut graph = StateGraph::<AgentState>::new();
    graph
        .add_node("echo", Arc::new(EchoAgent::new()))
        .add_edge(START, "echo")
        .add_edge("echo", END);

    let compiled = graph.with_store(store).compile().unwrap();
    assert!(compiled.store().is_some());
    let graph_store = compiled.store().unwrap().clone();
    let ns = vec!["u1".to_string(), "memories".to_string()];
    graph_store
        .put(&ns, "k1", &serde_json::json!("v1"))
        .await
        .unwrap();
    let v = graph_store.get(&ns, "k1").await.unwrap();
    assert_eq!(v.as_ref().and_then(|x| x.as_str()), Some("v1"));
}
