//! Integration test: ReAct linear chain think → act → observe → END.
//!
//! From User input to tool_results written back into messages; no real LLM/MCP.

mod init_logging;

use std::sync::Arc;

use graphweave::{
    ActNode, CompiledStateGraph, Message, MockLlm, MockToolSource, ObserveNode, ReActState,
    StateGraph, ThinkNode, END, START,
};

#[tokio::test]
async fn react_linear_chain_user_to_tool_result_in_messages() {
    let mut graph = StateGraph::<ReActState>::new();
    graph
        .add_node(
            "think",
            Arc::new(ThinkNode::new(Box::new(MockLlm::with_get_time_call()))),
        )
        .add_node(
            "act",
            Arc::new(ActNode::new(Box::new(MockToolSource::get_time_example()))),
        )
        .add_node("observe", Arc::new(ObserveNode::new()))
        .add_edge(START, "think")
        .add_edge("think", "act")
        .add_edge("act", "observe")
        .add_edge("observe", END);

    let compiled: CompiledStateGraph<ReActState> = graph.compile().expect("valid graph");

    let state = ReActState {
        messages: vec![Message::user("What time is it?")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
    };

    let out = compiled.invoke(state, None).await.unwrap();

    // think: 1 user -> 2 (user + assistant)
    // act: filled tool_results
    // observe: merged tool result as User message, cleared tool_*
    assert!(out.messages.len() >= 3);
    assert!(matches!(&out.messages[0], Message::User(_)));
    assert!(matches!(&out.messages[1], Message::Assistant(_)));
    assert!(
        matches!(&out.messages[2], Message::User(s) if s.contains("Tool") && s.contains("2025-01-29"))
    );
    assert!(out.tool_calls.is_empty());
    assert!(out.tool_results.is_empty());
}

/// Multi-round ReAct: first round think returns tool_calls, observe returns Node("think");
/// second round think returns no tool_calls, observe returns End.
#[tokio::test]
async fn react_multi_round_loop_then_end() {
    let mut graph = StateGraph::<ReActState>::new();
    graph
        .add_node(
            "think",
            Arc::new(ThinkNode::new(Box::new(MockLlm::first_tools_then_end()))),
        )
        .add_node(
            "act",
            Arc::new(ActNode::new(Box::new(MockToolSource::get_time_example()))),
        )
        .add_node("observe", Arc::new(ObserveNode::with_loop()))
        .add_edge(START, "think")
        .add_edge("think", "act")
        .add_edge("act", "observe")
        .add_edge("observe", END);

    let compiled: CompiledStateGraph<ReActState> = graph.compile().expect("valid graph");

    let state = ReActState {
        messages: vec![Message::user("What time is it?")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
    };

    let out = compiled.invoke(state, None).await.unwrap();

    // Round 1: user, assistant "I'll check.", tool result User message (3).
    // Round 2: think again (no tool_calls), assistant "The time is as above." (4); observe returns End.
    assert!(out.messages.len() >= 4);
    assert!(out.tool_calls.is_empty());
    assert!(out.tool_results.is_empty());
}
