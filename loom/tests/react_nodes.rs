//! Unit tests for ReAct nodes: ThinkNode, ActNode, ObserveNode.
//!
//! Each node is fed ReActState and we assert output state shape and content;
//! uses MockLlm and MockToolSource.

mod init_logging;

use std::collections::HashSet;
use std::sync::Arc;

use loom::{
    graph::RunContext,
    helve::ApprovalPolicy,
    memory::RunnableConfig,
    stream::{StreamEvent, StreamMode},
    tool_source::FileToolSource,
    ActNode, LlmUsage, Message, MockLlm, MockToolSource, Next, Node, ObserveNode, ReActState,
    STEP_PROGRESS_EVENT_TYPE, ThinkNode, ToolCall, ToolResult,
};
use tokio::sync::mpsc;

// --- ThinkNode ---

#[tokio::test]
async fn think_node_id_is_think() {
    let llm = MockLlm::with_get_time_call();
    let node = ThinkNode::new(Arc::new(llm));
    assert_eq!(node.id(), "think");
}

#[tokio::test]
async fn think_node_appends_assistant_message_and_sets_tool_calls() {
    let llm = MockLlm::with_get_time_call();
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("What time is it?")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, _) = node.run(state).await.unwrap();
    assert_eq!(out.messages.len(), 2);
    assert!(matches!(&out.messages[1], Message::Assistant(s) if s == "I'll check the time."));
    assert_eq!(out.tool_calls.len(), 1);
    assert_eq!(out.tool_calls[0].name, "get_time");
    assert_eq!(out.tool_calls[0].arguments, "{}");
    assert_eq!(out.tool_results.len(), 0);
}

#[tokio::test]
async fn think_node_with_no_tool_calls_sets_empty_tool_calls() {
    let llm = MockLlm::with_no_tool_calls("Hello.");
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("Hi")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, _) = node.run(state).await.unwrap();
    assert_eq!(out.messages.len(), 2);
    assert!(matches!(&out.messages[1], Message::Assistant(s) if s == "Hello."));
    assert!(out.tool_calls.is_empty());
    assert!(out.tool_results.is_empty());
}

#[tokio::test]
async fn think_node_preserves_tool_results_from_input_state() {
    let llm = MockLlm::with_no_tool_calls("Done.");
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("Hi")],
        tool_calls: vec![],
        tool_results: vec![ToolResult {
            call_id: Some("c1".into()),
            name: Some("get_time".into()),
            content: "12:00".into(),
            is_error: false,
        }],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, _) = node.run(state).await.unwrap();
    assert_eq!(out.tool_results.len(), 1);
    assert_eq!(out.tool_results[0].content, "12:00");
}

#[tokio::test]
async fn think_node_sets_message_count_after_last_think() {
    let llm = MockLlm::with_no_tool_calls("Hi.");
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("Hello")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, _) = node.run(state).await.unwrap();
    assert_eq!(out.messages.len(), 2);
    assert_eq!(out.message_count_after_last_think, Some(2));
}

#[tokio::test]
async fn think_node_usage_merge_none_plus_some() {
    let usage = LlmUsage {
        prompt_tokens: 10,
        completion_tokens: 5,
        total_tokens: 15,
    };
    let llm = MockLlm::with_no_tool_calls("Ok.").with_usage(usage.clone());
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("Hi")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, _) = node.run(state).await.unwrap();
    let u = out.usage.as_ref().expect("usage should be set");
    assert_eq!(u.prompt_tokens, 10);
    assert_eq!(u.completion_tokens, 5);
    assert_eq!(u.total_tokens, 15);
    let t = out.total_usage.as_ref().expect("total_usage should be set");
    assert_eq!(t.prompt_tokens, 10);
    assert_eq!(t.completion_tokens, 5);
    assert_eq!(t.total_tokens, 15);
}

#[tokio::test]
async fn think_node_usage_merge_some_plus_some() {
    let prev = LlmUsage {
        prompt_tokens: 10,
        completion_tokens: 5,
        total_tokens: 15,
    };
    let curr = LlmUsage {
        prompt_tokens: 20,
        completion_tokens: 8,
        total_tokens: 28,
    };
    let llm = MockLlm::with_no_tool_calls("Ok.").with_usage(curr);
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("Hi")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: Some(prev),
        message_count_after_last_think: None,
    };
    let (out, _) = node.run(state).await.unwrap();
    assert_eq!(out.usage.as_ref().map(|u| u.total_tokens), Some(28));
    assert_eq!(
        out.total_usage.as_ref().map(|u| u.total_tokens),
        Some(15 + 28)
    );
    assert_eq!(
        out.total_usage.as_ref().map(|u| u.prompt_tokens),
        Some(30)
    );
}

#[tokio::test]
async fn think_node_fallback_when_empty_content_and_no_tools() {
    let llm = MockLlm::with_no_tool_calls("");
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("Hi")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, _) = node.run_with_context(
        state,
        &RunContext::<ReActState> {
            config: RunnableConfig::default(),
            stream_tx: None,
            stream_mode: HashSet::new(),
            managed_values: Default::default(),
            store: None,
            previous: None,
            runtime_context: None,
        },
    )
    .await
    .unwrap();
    let expected = "No text response from the model. Please try again or check the API.";
    assert!(matches!(&out.messages[1], Message::Assistant(s) if s == expected));
}

#[tokio::test]
async fn think_node_fallback_streaming_emits_messages_event() {
    let llm = MockLlm::with_no_tool_calls("");
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("Hi")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (tx, mut rx) = mpsc::channel::<StreamEvent<ReActState>>(128);
    let ctx = RunContext::<ReActState> {
        config: RunnableConfig::default(),
        stream_tx: Some(tx),
        stream_mode: HashSet::from_iter([StreamMode::Messages]),
        managed_values: Default::default(),
        store: None,
        previous: None,
        runtime_context: None,
    };
    let (out, _) = node.run_with_context(state, &ctx).await.unwrap();
    drop(ctx);
    let mut events: Vec<_> = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    let expected = "No text response from the model. Please try again or check the API.";
    assert!(matches!(&out.messages[1], Message::Assistant(s) if s == expected));
    assert_eq!(events.len(), 1, "should emit one Messages event for fallback");
    match &events[0] {
        StreamEvent::Messages { chunk, metadata } => {
            assert_eq!(chunk.content, expected);
            assert_eq!(metadata.loom_node, "think");
        }
        _ => panic!("expected Messages event, got {:?}", events[0]),
    }
}

#[tokio::test]
async fn think_node_stream_emits_usage_when_available() {
    let usage = LlmUsage {
        prompt_tokens: 10,
        completion_tokens: 5,
        total_tokens: 15,
    };
    let llm = MockLlm::with_no_tool_calls("Hello").with_usage(usage).with_stream_by_char();
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("Hi")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (tx, mut rx) = mpsc::channel::<StreamEvent<ReActState>>(128);
    let ctx = RunContext::<ReActState> {
        config: RunnableConfig::default(),
        stream_tx: Some(tx),
        stream_mode: HashSet::from_iter([StreamMode::Messages]),
        managed_values: Default::default(),
        store: None,
        previous: None,
        runtime_context: None,
    };
    let _ = node.run_with_context(state, &ctx).await.unwrap();
    drop(ctx);
    let mut usage_events = 0u32;
    while let Ok(e) = rx.try_recv() {
        if let StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        } = e
        {
            usage_events += 1;
            assert_eq!(prompt_tokens, 10);
            assert_eq!(completion_tokens, 5);
            assert_eq!(total_tokens, 15);
        }
    }
    assert_eq!(usage_events, 1, "should emit exactly one Usage event");
}

// --- ActNode ---

#[tokio::test]
async fn act_node_id_is_act() {
    let tools = MockToolSource::get_time_example();
    let node = ActNode::new(Box::new(tools));
    assert_eq!(node.id(), "act");
}

#[tokio::test]
async fn act_node_executes_tool_calls_and_writes_tool_results() {
    let tools = MockToolSource::get_time_example();
    let node = ActNode::new(Box::new(tools));
    let state = ReActState {
        messages: vec![Message::user("What time?")],
        tool_calls: vec![ToolCall {
            name: "get_time".into(),
            arguments: "{}".into(),
            id: Some("call-1".into()),
        }],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, _) = node.run(state).await.unwrap();
    assert_eq!(out.messages.len(), 1);
    assert_eq!(out.tool_calls.len(), 1);
    assert_eq!(out.tool_results.len(), 1);
    assert_eq!(out.tool_results[0].call_id.as_deref(), Some("call-1"));
    assert_eq!(out.tool_results[0].name.as_deref(), Some("get_time"));
    assert_eq!(out.tool_results[0].content, "2025-01-29 12:00:00");
}

#[tokio::test]
async fn act_node_empty_tool_calls_leaves_tool_results_empty() {
    let tools = MockToolSource::get_time_example();
    let node = ActNode::new(Box::new(tools));
    let state = ReActState {
        messages: vec![Message::user("Hi")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, _) = node.run(state).await.unwrap();
    assert!(out.tool_results.is_empty());
    assert!(out.tool_calls.is_empty());
}

/// **Scenario**: ActNode run_with_context emits step_progress Custom events when StreamMode::Custom is enabled.
#[tokio::test]
async fn act_node_run_with_context_emits_step_progress_when_custom_mode() {
    let tools = MockToolSource::get_time_example();
    let node = ActNode::new(Box::new(tools));
    let state = ReActState {
        messages: vec![],
        tool_calls: vec![ToolCall {
            name: "get_time".into(),
            arguments: "{}".into(),
            id: Some("c1".into()),
        }],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };

    let (tx, mut rx) = mpsc::channel::<StreamEvent<ReActState>>(8);
    let config = RunnableConfig::default();
    let ctx = RunContext::<ReActState> {
        config,
        stream_tx: Some(tx),
        stream_mode: HashSet::from_iter([StreamMode::Custom]),
        managed_values: Default::default(),
        store: None,
        previous: None,
        runtime_context: None,
    };

    let (out, _) = node.run_with_context(state, &ctx).await.unwrap();
    assert_eq!(out.tool_results.len(), 1);
    assert!(out.tool_results[0].content.contains("2025"));

    drop(ctx);
    let mut customs = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let StreamEvent::Custom(v) = ev {
            customs.push(v);
        }
    }
    assert_eq!(
        customs.len(),
        1,
        "should emit one step_progress Custom event"
    );
    let payload = &customs[0];
    assert_eq!(
        payload.get("type").and_then(|v| v.as_str()),
        Some(STEP_PROGRESS_EVENT_TYPE)
    );
    assert_eq!(payload.get("node_id").and_then(|v| v.as_str()), Some("act"));
    assert_eq!(
        payload.get("tool_name").and_then(|v| v.as_str()),
        Some("get_time")
    );
    assert!(payload
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .contains("2025"));
}

/// **Scenario**: ActNode with approval_policy DestructiveOnly and delete_file tool_call
/// interrupts when approval_result is None; with approval_result Some(true) it executes.
#[tokio::test]
async fn act_node_approval_required_interrupts_then_executes_on_resume() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.txt");
    std::fs::write(&path, "content").unwrap();
    let source = FileToolSource::new(dir.path()).unwrap();
    let node =
        ActNode::new(Box::new(source)).with_approval_policy(Some(ApprovalPolicy::DestructiveOnly));

    let state = ReActState {
        messages: vec![],
        tool_calls: vec![ToolCall {
            name: "delete_file".into(),
            arguments: serde_json::json!({ "path": "x.txt" }).to_string(),
            id: Some("c1".into()),
        }],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };

    let err = node.run(state.clone()).await.unwrap_err();
    assert!(
        matches!(err, loom::AgentError::Interrupted(_)),
        "expected Interrupted, got {:?}",
        err
    );
    assert!(path.exists(), "file must still exist before approval");

    let state_approved = ReActState {
        approval_result: Some(true),
        ..state
    };
    let (out, _) = node.run(state_approved).await.unwrap();
    assert_eq!(out.tool_results.len(), 1);
    assert_eq!(out.tool_results[0].content, "ok");
    assert!(!path.exists(), "file should be deleted after approval");
}

#[tokio::test]
async fn act_node_multiple_tool_calls_produces_multiple_results() {
    let tools = MockToolSource::get_time_example();
    let node = ActNode::new(Box::new(tools));
    let state = ReActState {
        messages: vec![],
        tool_calls: vec![
            ToolCall {
                name: "get_time".into(),
                arguments: "{}".into(),
                id: Some("c1".into()),
            },
            ToolCall {
                name: "get_time".into(),
                arguments: r#"{}"#.into(),
                id: Some("c2".into()),
            },
        ],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, _) = node.run(state).await.unwrap();
    assert_eq!(out.tool_results.len(), 2);
    assert_eq!(out.tool_results[0].content, "2025-01-29 12:00:00");
    assert_eq!(out.tool_results[1].content, "2025-01-29 12:00:00");
}

// --- ObserveNode ---

#[tokio::test]
async fn observe_node_id_is_observe() {
    let node = ObserveNode::new();
    assert_eq!(node.id(), "observe");
}

#[tokio::test]
async fn observe_node_appends_tool_results_as_user_messages_and_clears_tool_fields() {
    let node = ObserveNode::new();
    let state = ReActState {
        messages: vec![
            Message::user("What time?"),
            Message::Assistant("I'll check.".into()),
        ],
        tool_calls: vec![ToolCall {
            name: "get_time".into(),
            arguments: "{}".into(),
            id: Some("call-1".into()),
        }],
        tool_results: vec![ToolResult {
            call_id: Some("call-1".into()),
            name: Some("get_time".into()),
            content: "2025-01-29 12:00:00".into(),
            is_error: false,
        }],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, _) = node.run(state).await.unwrap();
    assert_eq!(out.messages.len(), 3);
    assert!(
        matches!(&out.messages[2], Message::User(s) if s.contains("Tool") && s.contains("2025-01-29 12:00:00"))
    );
    assert!(out.tool_calls.is_empty());
    assert!(out.tool_results.is_empty());
}

#[tokio::test]
async fn observe_node_empty_tool_results_clears_tool_fields_only() {
    let node = ObserveNode::new();
    let state = ReActState {
        messages: vec![Message::user("Hi"), Message::Assistant("Hello.".into())],
        tool_calls: vec![ToolCall {
            name: "x".into(),
            arguments: "{}".into(),
            id: None,
        }],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, _) = node.run(state).await.unwrap();
    assert_eq!(out.messages.len(), 2);
    assert!(out.tool_calls.is_empty());
    assert!(out.tool_results.is_empty());
}

#[tokio::test]
async fn observe_node_default_constructible() {
    let node = ObserveNode::default();
    assert_eq!(node.id(), "observe");
}

#[tokio::test]
async fn observe_node_with_loop_returns_node_think_when_had_tool_calls() {
    let node = ObserveNode::with_loop();
    let state = ReActState {
        messages: vec![
            Message::user("Hi"),
            Message::Assistant("I'll check.".into()),
        ],
        tool_calls: vec![ToolCall {
            name: "get_time".into(),
            arguments: "{}".into(),
            id: Some("c1".into()),
        }],
        tool_results: vec![ToolResult {
            call_id: Some("c1".into()),
            name: Some("get_time".into()),
            content: "12:00".into(),
            is_error: false,
        }],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, next) = node.run(state).await.unwrap();
    assert_eq!(out.messages.len(), 3);
    // With compression, observe returns Continue so the graph follows observe → compress → think.
    assert!(matches!(next, Next::Continue));
}

#[tokio::test]
async fn observe_node_with_loop_returns_end_when_no_tool_calls() {
    let node = ObserveNode::with_loop();
    let state = ReActState {
        messages: vec![Message::user("Hi"), Message::Assistant("Hello.".into())],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, next) = node.run(state).await.unwrap();
    assert_eq!(out.messages.len(), 2);
    assert!(matches!(next, Next::End));
}

/// **Scenario**: When enable_loop and turn_count reaches max (10), observe returns End even if there were tool_calls.
#[tokio::test]
async fn observe_node_with_loop_returns_end_when_max_turns_reached() {
    const MAX_TURNS: u32 = 10;

    let node = ObserveNode::with_loop();
    let state = ReActState {
        messages: vec![
            Message::user("Hi"),
            Message::Assistant("I'll check.".into()),
        ],
        tool_calls: vec![ToolCall {
            name: "get_time".into(),
            arguments: "{}".into(),
            id: Some("c1".into()),
        }],
        tool_results: vec![ToolResult {
            call_id: Some("c1".into()),
            name: Some("get_time".into()),
            content: "12:00".into(),
            is_error: false,
        }],
        turn_count: MAX_TURNS - 1,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };
    let (out, next) = node.run(state).await.unwrap();
    assert_eq!(out.messages.len(), 3);
    assert_eq!(out.turn_count, MAX_TURNS);
    assert!(matches!(next, Next::End));
}

// --- ThinkNode Messages Streaming ---

/// **Scenario**: ThinkNode emits Messages when stream_mode contains Messages.
#[tokio::test]
async fn think_node_run_with_context_emits_messages_when_streaming() {
    let content = "Hello world";
    let llm = MockLlm::with_no_tool_calls(content).with_stream_by_char();
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("Hi")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };

    // Create stream channel
    let (tx, mut rx) = mpsc::channel::<StreamEvent<ReActState>>(128);

    // Create RunContext with Messages streaming enabled
    let config = RunnableConfig::default();
    let ctx = RunContext::<ReActState> {
        config,
        stream_tx: Some(tx),
        stream_mode: HashSet::from_iter([StreamMode::Messages]),
        managed_values: Default::default(),
        store: None,
        previous: None,
        runtime_context: None,
    };

    // Run node with context
    let (out, _) = node.run_with_context(state, &ctx).await.unwrap();

    // Verify output state
    assert_eq!(out.messages.len(), 2);
    assert!(matches!(&out.messages[1], Message::Assistant(s) if s == content));

    // Collect stream events
    drop(ctx); // Drop ctx to close channel
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }

    // Verify Messages events were emitted (one per character)
    assert_eq!(
        events.len(),
        content.len(),
        "should emit one Messages event per character"
    );
    for (i, event) in events.iter().enumerate() {
        match event {
            StreamEvent::Messages { chunk, metadata } => {
                assert_eq!(
                    chunk.content,
                    content.chars().nth(i).unwrap().to_string(),
                    "chunk content should be character at index {}",
                    i
                );
                assert_eq!(
                    metadata.loom_node, "think",
                    "metadata should indicate think node"
                );
            }
            _ => panic!("expected Messages event, got {:?}", event),
        }
    }
}

/// **Scenario**: ThinkNode does NOT emit Messages when stream_mode does not contain Messages.
#[tokio::test]
async fn think_node_run_with_context_no_messages_when_mode_empty() {
    let content = "Hello world";
    let llm = MockLlm::with_no_tool_calls(content).with_stream_by_char();
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("Hi")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };

    // Create stream channel
    let (tx, mut rx) = mpsc::channel::<StreamEvent<ReActState>>(128);

    // Create RunContext WITHOUT Messages in stream_mode
    let config = RunnableConfig::default();
    let ctx = RunContext::<ReActState> {
        config,
        stream_tx: Some(tx),
        stream_mode: HashSet::from_iter([StreamMode::Values]), // Messages not included
        managed_values: Default::default(),
        store: None,
        previous: None,
        runtime_context: None,
    };

    // Run node with context
    let (out, _) = node.run_with_context(state, &ctx).await.unwrap();

    // Verify output state is correct
    assert_eq!(out.messages.len(), 2);
    assert!(matches!(&out.messages[1], Message::Assistant(s) if s == content));

    // Verify NO Messages events were emitted
    drop(ctx);
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    assert!(
        events.is_empty(),
        "should not emit any events when Messages not in stream_mode"
    );
}

/// **Scenario**: ThinkNode run_with_context works when stream_tx is None.
#[tokio::test]
async fn think_node_run_with_context_no_panic_when_no_stream_tx() {
    let content = "Hello";
    let llm = MockLlm::with_no_tool_calls(content);
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("Hi")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };

    // Create RunContext without stream_tx
    let config = RunnableConfig::default();
    let ctx = RunContext::<ReActState> {
        config,
        stream_tx: None,
        stream_mode: HashSet::from_iter([StreamMode::Messages]),
        managed_values: Default::default(),
        store: None,
        previous: None,
        runtime_context: None,
    };

    // Should complete without panic
    let (out, _) = node.run_with_context(state, &ctx).await.unwrap();
    assert_eq!(out.messages.len(), 2);
}

/// **Scenario**: ThinkNode streams concatenated chunks equal full content.
#[tokio::test]
async fn think_node_stream_chunks_concatenate_to_full_content() {
    let content = "Test streaming message";
    let llm = MockLlm::with_no_tool_calls(content).with_stream_by_char();
    let node = ThinkNode::new(Arc::new(llm));
    let state = ReActState {
        messages: vec![Message::user("Hi")],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };

    let (tx, mut rx) = mpsc::channel::<StreamEvent<ReActState>>(128);
    let config = RunnableConfig::default();
    let ctx = RunContext::<ReActState> {
        config,
        stream_tx: Some(tx),
        stream_mode: HashSet::from_iter([StreamMode::Messages]),
        managed_values: Default::default(),
        store: None,
        previous: None,
        runtime_context: None,
    };

    let (out, _) = node.run_with_context(state, &ctx).await.unwrap();

    // Collect and concatenate chunks
    drop(ctx);
    let mut concatenated = String::new();
    while let Ok(event) = rx.try_recv() {
        if let StreamEvent::Messages { chunk, .. } = event {
            concatenated.push_str(&chunk.content);
        }
    }

    // Verify concatenated equals original content and assistant message
    assert_eq!(concatenated, content);
    assert!(matches!(&out.messages[1], Message::Assistant(s) if s == content));
}
