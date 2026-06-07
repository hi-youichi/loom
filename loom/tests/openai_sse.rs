//! Unit tests for OpenAI SSE adapter: StreamEvent → SSE lines.
//!
//! **Scenario**: Given a fixed sequence of StreamEvent (TaskStart think → Messages → TaskEnd → Values),
//! the adapter emits SSE lines that match OpenAI chat.completion.chunk format: first line with
//! role+content, content deltas, then finish() yields final chunk with finish_reason "stop".

mod init_logging;

use loom_stream::{MessageChunk, StreamMetadata};
use loom_types::state::ReActState;
use loom::{ChunkMeta, ModelConfig, StreamEvent, StreamToSse};

fn empty_state() -> ReActState {
    ReActState {
        model_config: ModelConfig::default(),
        messages: vec![],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
        last_reasoning_content: None,
        think_count: 0,
        summary: None,
        should_continue: true,
        force_compact: false,
    }
}

/// **Scenario**: First event TaskStart(think) produces one SSE line with role "assistant" and content "".
#[test]
fn adapter_emits_initial_chunk_on_task_start_think() {
    let meta = ChunkMeta {
        id: "chatcmpl-test1".to_string(),
        model: "gpt-4o".to_string(),
        created: Some(1694268190),
    };
    let mut adapter = StreamToSse::new(meta, false);

    adapter.feed(StreamEvent::TaskStart {
        node_id: "think".to_string(),
        namespace: None,
    });

    let lines = adapter.take_lines();
    assert_eq!(lines.len(), 1, "one SSE line for initial chunk");
    assert!(lines[0].starts_with("data: "));
    assert!(lines[0].ends_with("\n\n"));
    assert!(lines[0].contains(r#""role":"assistant""#));
    assert!(lines[0].contains(r#""content":""#));
    assert!(lines[0].contains(r#""object":"chat.completion.chunk""#));
    // finish_reason may be omitted when null (serde skip_serializing_if) or present as null
}

/// **Scenario**: Messages events produce one SSE line per chunk with content delta.
#[test]
fn adapter_emits_content_delta_per_messages_event() {
    let meta = ChunkMeta {
        id: "chatcmpl-test2".to_string(),
        model: "gpt-4o".to_string(),
        created: Some(1694268190),
    };
    let mut adapter = StreamToSse::new(meta, false);

    adapter.feed(StreamEvent::TaskStart {
        node_id: "think".to_string(),
        namespace: None,
    });
    adapter.feed(StreamEvent::Messages {
        chunk: MessageChunk::message("Hello"),
        metadata: StreamMetadata {
            loom_node: "think".to_string(),
            namespace: None,
        },
    });
    adapter.feed(StreamEvent::Messages {
        chunk: MessageChunk::message(" world"),
        metadata: StreamMetadata {
            loom_node: "think".to_string(),
            namespace: None,
        },
    });

    let lines = adapter.take_lines();
    assert_eq!(lines.len(), 3, "initial + two content chunks");
    assert!(lines[1].contains(r#""content":"Hello""#));
    assert!(lines[2].contains(r#""content":" world""#));
}

/// **Scenario**: finish() emits final chunk with finish_reason "stop" and no content delta.
#[test]
fn adapter_finish_emits_stop_chunk() {
    let meta = ChunkMeta {
        id: "chatcmpl-test3".to_string(),
        model: "gpt-4o".to_string(),
        created: Some(1694268190),
    };
    let mut adapter = StreamToSse::new(meta, false);

    adapter.feed(StreamEvent::TaskStart {
        node_id: "think".to_string(),
        namespace: None,
    });
    adapter.feed(StreamEvent::Messages {
        chunk: MessageChunk::message("Hi"),
        metadata: StreamMetadata {
            loom_node: "think".to_string(),
            namespace: None,
        },
    });
    adapter.finish();

    let lines = adapter.take_lines();
    let last = lines.last().expect("at least one line");
    assert!(last.contains(r#""finish_reason":"stop""#));
    assert!(last.contains(r#""object":"chat.completion.chunk""#));
}

/// **Scenario**: When include_usage is true and Usage was fed, final chunk includes usage.
#[test]
fn adapter_finish_includes_usage_when_requested() {
    let meta = ChunkMeta {
        id: "chatcmpl-test4".to_string(),
        model: "gpt-4o".to_string(),
        created: Some(1694268190),
    };
    let mut adapter = StreamToSse::new(meta, true);

    adapter.feed(StreamEvent::TaskStart {
        node_id: "think".to_string(),
        namespace: None,
    });
    adapter.feed(StreamEvent::Usage {
        prompt_tokens: 10,
        completion_tokens: 5,
        total_tokens: 15,
        prefill_duration: None,
        decode_duration: None,
    });
    adapter.finish();

    let lines = adapter.take_lines();
    let last = lines.last().expect("at least one line");
    assert!(last.contains(r#""usage""#));
    assert!(last.contains(r#""prompt_tokens":10"#));
    assert!(last.contains(r#""completion_tokens":5"#));
    assert!(last.contains(r#""total_tokens":15"#));
}

/// **Scenario**: new_with_sink sends each line to the channel as it is produced.
#[tokio::test]
async fn adapter_with_sink_sends_lines_to_channel() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(16);
    let meta = ChunkMeta {
        id: "chatcmpl-sink".to_string(),
        model: "gpt-4o".to_string(),
        created: Some(1694268190),
    };
    let mut adapter = StreamToSse::new_with_sink(meta, false, tx);

    adapter.feed(StreamEvent::TaskStart {
        node_id: "think".to_string(),
        namespace: None,
    });
    let first = rx.recv().await.expect("one line for initial chunk");
    assert!(first.starts_with("data: "));
    assert!(first.contains(r#""role":"assistant""#));

    adapter.feed(StreamEvent::Messages {
        chunk: MessageChunk::message("Hi"),
        metadata: StreamMetadata {
            loom_node: "think".to_string(),
            namespace: None,
        },
    });
    let second = rx.recv().await.expect("one line for content");
    assert!(second.contains(r#""content":"Hi""#));

    adapter.finish();
    let third = rx.recv().await.expect("one line for stop");
    assert!(third.contains(r#""finish_reason":"stop""#));

    drop(adapter);
    assert!(rx.recv().await.is_none());
}

/// **Scenario**: Updates with non-empty tool_calls emits a chunk with delta.tool_calls and finish_reason "tool_calls".
#[test]
fn adapter_emits_tool_calls_chunk_on_updates_with_tool_calls() {
    use loom_llm::ToolCall;

    let meta = ChunkMeta {
        id: "chatcmpl-tc".to_string(),
        model: "gpt-4o".to_string(),
        created: Some(1694268190),
    };
    let mut adapter = StreamToSse::new(meta, false);

    adapter.feed(StreamEvent::Updates {
        node_id: "act".to_string(),
        state: ReActState {
            model_config: ModelConfig::default(),
            messages: vec![],
            tool_calls: vec![
                ToolCall {
                    id: Some("call_1".to_string()),
                    name: "get_time".to_string(),
                    arguments: "{}".to_string(),
                },
                ToolCall {
                    id: None,
                    name: "search".to_string(),
                    arguments: r#"{"q":"x"}"#.to_string(),
                },
            ],
            tool_results: vec![],
            turn_count: 0,
            approval_result: None,
            usage: None,
            total_usage: None,
            message_count_after_last_think: None,
            last_reasoning_content: None,
            think_count: 0,
            summary: None,
            should_continue: true,
        force_compact: false,
        },
        namespace: None,
    });

    let lines = adapter.take_lines();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains(r#""finish_reason":"tool_calls""#));
    assert!(lines[0].contains(r#""tool_calls""#));
    assert!(lines[0].contains("get_time"));
    assert!(lines[0].contains("search"));
    assert!(lines[0].contains("call_1"));
}

/// **Scenario**: Values event does not emit a chunk; only finish() emits the final chunk.
#[test]
fn adapter_values_does_not_emit_finish_chunk() {
    let meta = ChunkMeta {
        id: "chatcmpl-test5".to_string(),
        model: "gpt-4o".to_string(),
        created: Some(1694268190),
    };
    let mut adapter = StreamToSse::new(meta, false);

    adapter.feed(StreamEvent::TaskStart {
        node_id: "think".to_string(),
        namespace: None,
    });
    adapter.feed(StreamEvent::Values(empty_state()));
    adapter.feed(StreamEvent::Values(empty_state()));

    let lines = adapter.take_lines();
    assert_eq!(
        lines.len(),
        1,
        "only initial chunk; no finish until finish()"
    );
    adapter.finish();
    let lines2 = adapter.take_lines();
    assert_eq!(lines2.len(), 1, "finish adds one final chunk");
}
