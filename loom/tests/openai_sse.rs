//! Unit tests for OpenAI SSE adapter: StreamEvent → SSE lines and request parsing.
//!
//! **Scenario**: Given a fixed sequence of StreamEvent (TaskStart think → Messages → TaskEnd → Values),
//! the adapter emits SSE lines that match OpenAI chat.completion.chunk format: first line with
//! role+content, content deltas, then finish() yields final chunk with finish_reason "stop".
//! **Scenario**: parse_chat_request extracts user_message, system_prompt, runnable_config from request.

mod init_logging;

use loom::{
    parse_chat_request,
    stream::{MessageChunk, StreamMetadata},
    ChatCompletionRequest, ChatMessage, ChunkMeta, ReActState, StreamEvent, StreamToSse,
};

fn empty_state() -> ReActState {
    ReActState {
        messages: vec![],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
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
    });
    adapter.feed(StreamEvent::Messages {
        chunk: MessageChunk {
            content: "Hello".to_string(),
        },
        metadata: StreamMetadata {
            loom_node: "think".to_string(),
        },
    });
    adapter.feed(StreamEvent::Messages {
        chunk: MessageChunk {
            content: " world".to_string(),
        },
        metadata: StreamMetadata {
            loom_node: "think".to_string(),
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
    });
    adapter.feed(StreamEvent::Messages {
        chunk: MessageChunk {
            content: "Hi".to_string(),
        },
        metadata: StreamMetadata {
            loom_node: "think".to_string(),
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
    });
    adapter.feed(StreamEvent::Usage {
        prompt_tokens: 10,
        completion_tokens: 5,
        total_tokens: 15,
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
    });
    let first = rx.recv().await.expect("one line for initial chunk");
    assert!(first.starts_with("data: "));
    assert!(first.contains(r#""role":"assistant""#));

    adapter.feed(StreamEvent::Messages {
        chunk: MessageChunk {
            content: "Hi".to_string(),
        },
        metadata: StreamMetadata {
            loom_node: "think".to_string(),
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
    use loom::ToolCall;

    let meta = ChunkMeta {
        id: "chatcmpl-tc".to_string(),
        model: "gpt-4o".to_string(),
        created: Some(1694268190),
    };
    let mut adapter = StreamToSse::new(meta, false);

    adapter.feed(StreamEvent::Updates {
        node_id: "act".to_string(),
        state: ReActState {
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
        },
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

// --- parse_chat_request ---

/// **Scenario**: parse_chat_request returns last user message and system prompt or default.
#[test]
fn parse_request_extracts_user_message_and_system_prompt() {
    let req = ChatCompletionRequest {
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: Some("You are helpful.".to_string().into()),
            },
            ChatMessage {
                role: "user".to_string(),
                content: Some("Hello".to_string().into()),
            },
        ],
        model: "gpt-4o".to_string(),
        stream: true,
        stream_options: None,
        thread_id: None,
        working_folder: None,
        approval_policy: None,
    };
    let parsed = parse_chat_request(&req).unwrap();
    assert_eq!(parsed.user_message, "Hello");
    assert_eq!(parsed.system_prompt, "You are helpful.");
    assert!(parsed.runnable_config.thread_id.is_none());
}

/// **Scenario**: When no system message, system_prompt is REACT_SYSTEM_PROMPT (code default).
#[test]
fn parse_request_uses_default_system_prompt_when_no_system_message() {
    let req = ChatCompletionRequest {
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: Some("Hi".to_string().into()),
        }],
        model: "gpt-4o".to_string(),
        stream: true,
        stream_options: None,
        thread_id: None,
        working_folder: None,
        approval_policy: None,
    };
    let parsed = parse_chat_request(&req).unwrap();
    assert_eq!(parsed.user_message, "Hi");
    assert_eq!(parsed.system_prompt, loom::agent::react::REACT_SYSTEM_PROMPT);
}

/// **Scenario**: thread_id in request is reflected in runnable_config.
#[test]
fn parse_request_passes_thread_id_to_runnable_config() {
    let req = ChatCompletionRequest {
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: Some("Hi".to_string().into()),
        }],
        model: "gpt-4o".to_string(),
        stream: true,
        stream_options: None,
        thread_id: Some("thread-123".to_string()),
        working_folder: None,
        approval_policy: None,
    };
    let parsed = parse_chat_request(&req).unwrap();
    assert_eq!(
        parsed.runnable_config.thread_id.as_deref(),
        Some("thread-123")
    );
}

/// **Scenario**: No user message returns ParseError::NoUserMessage.
#[test]
fn parse_request_errors_when_no_user_message() {
    let req = ChatCompletionRequest {
        messages: vec![ChatMessage {
            role: "system".to_string(),
            content: Some("Only system.".to_string().into()),
        }],
        model: "gpt-4o".to_string(),
        stream: true,
        stream_options: None,
        thread_id: None,
        working_folder: None,
        approval_policy: None,
    };
    let err = parse_chat_request(&req).unwrap_err();
    assert!(matches!(err, loom::ParseError::NoUserMessage));
}

/// **Scenario**: working_folder and approval_policy in request yield helve_config.
#[test]
fn parse_request_with_working_folder_and_approval_policy_sets_helve_config() {
    let dir = tempfile::tempdir().unwrap();
    let req = ChatCompletionRequest {
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: Some("List files.".to_string().into()),
        }],
        model: "gpt-4o".to_string(),
        stream: true,
        stream_options: None,
        thread_id: Some("t1".to_string()),
        working_folder: Some(dir.path().to_string_lossy().into_owned()),
        approval_policy: Some("destructive_only".to_string()),
    };
    let parsed = parse_chat_request(&req).unwrap();
    let helve = parsed.helve_config.as_ref().unwrap();
    assert!(helve.working_folder.as_ref().unwrap().exists());
    assert_eq!(
        helve.approval_policy,
        Some(loom::ApprovalPolicy::DestructiveOnly)
    );
    assert_eq!(helve.thread_id.as_deref(), Some("t1"));
}

/// **Scenario**: Invalid approval_policy returns ParseError::InvalidApprovalPolicy.
#[test]
fn parse_request_invalid_approval_policy_errors() {
    let req = ChatCompletionRequest {
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: Some("Hi".to_string().into()),
        }],
        model: "gpt-4o".to_string(),
        stream: true,
        stream_options: None,
        thread_id: None,
        working_folder: None,
        approval_policy: Some("invalid".to_string()),
    };
    let err = parse_chat_request(&req).unwrap_err();
    assert!(matches!(
        err,
        loom::ParseError::InvalidApprovalPolicy(_)
    ));
}
