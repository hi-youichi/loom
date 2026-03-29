//! Tests for CompletionCheckNode

use std::sync::Arc;
use loom::agent::react::CompletionCheckNode;
use loom::graph::{Node, RunContext};
use loom::llm::MockLlm;
use loom::memory::RunnableConfig;
use loom::message::Message;
use loom::state::ReActState;

#[tokio::test]
async fn completion_check_with_empty_state() {
    let llm = MockLlm::with_no_tool_calls("");
    let node = CompletionCheckNode::new(Arc::new(llm))
        .with_max_iterations(10)
        .with_message_window(5);

    let state = ReActState::default();
    let ctx = RunContext::<ReActState>::new(RunnableConfig::default());
    
    let (new_state, next) = node.run_with_context(state, &ctx).await.unwrap();
    
    // Empty state should end
    assert!(matches!(next, loom::graph::Next::End));
}

#[tokio::test]
async fn completion_check_respects_max_iterations() {
    let llm = MockLlm::with_no_tool_calls(r#"{"completed": false, "reason": "Still working"}"#);
    let node = CompletionCheckNode::new(Arc::new(llm))
        .with_max_iterations(3)
        .with_message_window(5);
    
    let state = ReActState {
        messages: vec![Message::user("Do something")],
        turn_count: 3, // Already at max
        ..Default::default()
    };
    let ctx = RunContext::<ReActState>::new(RunnableConfig::default());
    
    let (new_state, next) = node.run_with_context(state, &ctx).await.unwrap();
    
    // Should end due to max iterations
    assert!(matches!(next, loom::graph::Next::End));
}

#[tokio::test]
async fn completion_check_continues_when_incomplete() {
    let llm = MockLlm::with_no_tool_calls(r#"{"completed": false, "reason": "Need to do more work"}"#);
    let node = CompletionCheckNode::new(Arc::new(llm))
        .with_max_iterations(10)
        .with_message_window(5);
    
    let state = ReActState {
        messages: vec![
            Message::user("List files"),
            Message::assistant("I found 5 files"),
        ],
        turn_count: 1,
        ..Default::default()
    };
    let ctx = RunContext::<ReActState>::new(RunnableConfig::default());
    
    let (new_state, next) = node.run_with_context(state, &ctx).await.unwrap();
    
    // Should continue and set should_continue flag
    assert!(matches!(next, loom::graph::Next::Continue));
    assert!(new_state.should_continue);
    assert_eq!(new_state.turn_count, 2); // Incremented
}

#[tokio::test]
async fn completion_check_ends_when_complete() {
    let llm = MockLlm::with_no_tool_calls(r#"{"completed": true, "reason": "Task finished successfully"}"#);
    let node = CompletionCheckNode::new(Arc::new(llm))
        .with_max_iterations(10)
        .with_message_window(5);
    
    let state = ReActState {
        messages: vec![
            Message::user("What is 2+2?"),
            Message::assistant("The answer is 4"),
        ],
        turn_count: 1,
        ..Default::default()
    };
    let ctx = RunContext::<ReActState>::new(RunnableConfig::default());
    
    let (new_state, next) = node.run_with_context(state, &ctx).await.unwrap();
    
    // Should end
    assert!(matches!(next, loom::graph::Next::End));
    assert!(!new_state.should_continue);
}
