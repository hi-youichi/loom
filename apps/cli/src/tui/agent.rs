//! Agent event adapter layer for the TUI.
//!
//! Provides an [`AgentEvent`] enum that represents high-level streaming events
//! from the agent system, a channel factory, and a callback converter that
//! bridges [`stream_event::StreamEvent`] into the TUI event channel.
//!
//! This is the primary integration point between the agent runtime and the
//! TUI rendering loop. The TUI `App` spawns a background task that runs
//! the agent, and receives [`AgentEvent`]s via the receiver side of the
//! channel — driving inline rendering of text deltas, reasoning content,
//! tool call lifecycle, and completion/error states.

use serde_json::Value;
use stream_event::StreamEvent;
use tokio::sync::mpsc;

/// Agent event type for the TUI event loop.
///
/// Each variant maps to one or more [`StreamEvent`] variants, providing a
/// TUI-friendly representation that the rendering layer can consume directly.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Streaming text delta (the agent's reply, chunk by chunk).
    TextDelta(String),

    /// Streaming reasoning / thinking delta.
    ReasoningDelta(String),

    /// LLM decided to call a tool (complete arguments available).
    ToolCall {
        call_id: Option<String>,
        name: String,
        arguments: Value,
    },

    /// Tool execution started.
    ToolStart {
        call_id: Option<String>,
        name: String,
    },

    /// Incremental tool output during execution.
    ToolOutput {
        call_id: Option<String>,
        name: String,
        content: String,
    },

    /// Tool execution finished.
    ToolEnd {
        call_id: Option<String>,
        name: String,
        result: String,
        is_error: bool,
        raw_result: Option<String>,
    },

    /// Stream completed successfully (no more events expected).
    Completed,

    /// An error occurred during streaming.
    Error(String),
}

/// Create a new agent event channel with a buffer of 256 events.
///
/// Returns `(sender, receiver)` — the sender is passed to
/// [`create_stream_callback`] or to a background agent task, and the receiver
/// is consumed by the TUI event loop to drive inline rendering.
pub fn create_agent_channel() -> (mpsc::Sender<AgentEvent>, mpsc::Receiver<AgentEvent>) {
    mpsc::channel(256)
}

/// Create a callback that converts [`StreamEvent<Value>`] into [`AgentEvent`]
/// and sends them into the given channel via [`mpsc::Sender::try_send`].
///
/// The returned closure implements `FnMut(StreamEvent<Value>) + Send` and can
/// be passed as the stream event callback to the agent runtime. Events that
/// do not map to an [`AgentEvent`] variant (e.g. `TaskStart`, `Updates`,
/// `TurnFinish`) are silently dropped — they are not relevant to the TUI
/// rendering layer.
///
/// # Mapping
///
/// | `StreamEvent` variant | `AgentEvent` variant |
/// |---|---|
/// | `TextDelta { content, .. }` | `TextDelta(content)` |
/// | `ReasoningDelta { content, .. }` | `ReasoningDelta(content)` |
/// | `ToolCall { call_id, name, arguments }` | `ToolCall { call_id, name, arguments }` |
/// | `ToolStart { call_id, name }` | `ToolStart { call_id, name }` |
/// | `ToolOutput { call_id, name, content }` | `ToolOutput { call_id, name, content }` |
/// | `ToolEnd { call_id, name, result, is_error, raw_result }` | `ToolEnd { call_id, name, result, is_error, raw_result }` |
/// | `Finish` | `Completed` |
/// | `ProviderError { message }` | `Error(message)` |
/// | `ToolError { error, .. }` | `Error(error)` |
/// | All others | silently dropped |
pub fn create_stream_callback(
    tx: mpsc::Sender<AgentEvent>,
) -> impl FnMut(StreamEvent<Value>) + Send {
    move |event: StreamEvent<Value>| {
        let agent_event = match event {
            StreamEvent::TextDelta { content, .. } => AgentEvent::TextDelta(content),
            StreamEvent::ReasoningDelta { content, .. } => AgentEvent::ReasoningDelta(content),
            StreamEvent::ToolCall {
                call_id,
                name,
                arguments,
            } => AgentEvent::ToolCall {
                call_id,
                name,
                arguments,
            },
            StreamEvent::ToolStart { call_id, name } => AgentEvent::ToolStart { call_id, name },
            StreamEvent::ToolOutput {
                call_id,
                name,
                content,
            } => AgentEvent::ToolOutput {
                call_id,
                name,
                content,
            },
            StreamEvent::ToolEnd {
                call_id,
                name,
                result,
                is_error,
                raw_result,
            } => AgentEvent::ToolEnd {
                call_id,
                name,
                result,
                is_error,
                raw_result,
            },
            StreamEvent::Finish => AgentEvent::Completed,
            StreamEvent::ProviderError { message } => AgentEvent::Error(message),
            StreamEvent::ToolError { error, .. } => AgentEvent::Error(error),
            // All other stream events are not relevant to the TUI rendering layer.
            _ => return,
        };
        let _ = tx.try_send(agent_event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper: create a channel and callback, then dispatch a single event,
    /// returning the received [`AgentEvent`].
    fn dispatch(event: StreamEvent<Value>) -> AgentEvent {
        let (tx, mut rx) = create_agent_channel();
        let mut cb = create_stream_callback(tx);
        cb(event);
        rx.try_recv().expect("expected an AgentEvent")
    }

    /// Helper: dispatch an event and assert the channel is empty (event was
    /// silently dropped).
    fn assert_dropped(event: StreamEvent<Value>) {
        let (tx, mut rx) = create_agent_channel();
        let mut cb = create_stream_callback(tx);
        cb(event);
        assert!(
            rx.try_recv().is_err(),
            "expected event to be silently dropped"
        );
    }

    #[test]
    fn test_text_delta() {
        let ev = dispatch(StreamEvent::TextDelta {
            content: "hello".into(),
            metadata: Default::default(),
        });
        assert!(matches!(ev, AgentEvent::TextDelta(s) if s == "hello"));
    }

    #[test]
    fn test_reasoning_delta() {
        let ev = dispatch(StreamEvent::ReasoningDelta {
            id: "r1".into(),
            content: "thinking...".into(),
            metadata: Default::default(),
        });
        assert!(matches!(ev, AgentEvent::ReasoningDelta(s) if s == "thinking..."));
    }

    #[test]
    fn test_tool_call() {
        let ev = dispatch(StreamEvent::ToolCall {
            call_id: Some("c1".into()),
            name: "bash".into(),
            arguments: json!({"cmd": "ls"}),
        });
        match ev {
            AgentEvent::ToolCall {
                call_id,
                name,
                arguments,
            } => {
                assert_eq!(call_id, Some("c1".into()));
                assert_eq!(name, "bash");
                assert_eq!(arguments, json!({"cmd": "ls"}));
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn test_tool_start() {
        let ev = dispatch(StreamEvent::ToolStart {
            call_id: Some("c1".into()),
            name: "bash".into(),
        });
        match ev {
            AgentEvent::ToolStart { call_id, name } => {
                assert_eq!(call_id, Some("c1".into()));
                assert_eq!(name, "bash");
            }
            _ => panic!("expected ToolStart"),
        }
    }

    #[test]
    fn test_tool_output() {
        let ev = dispatch(StreamEvent::ToolOutput {
            call_id: Some("c1".into()),
            name: "bash".into(),
            content: "file.txt".into(),
        });
        match ev {
            AgentEvent::ToolOutput {
                call_id,
                name,
                content,
            } => {
                assert_eq!(call_id, Some("c1".into()));
                assert_eq!(name, "bash");
                assert_eq!(content, "file.txt");
            }
            _ => panic!("expected ToolOutput"),
        }
    }

    #[test]
    fn test_tool_end() {
        let ev = dispatch(StreamEvent::ToolEnd {
            call_id: Some("c1".into()),
            name: "bash".into(),
            result: "ok".into(),
            is_error: false,
            raw_result: Some("full output".into()),
        });
        match ev {
            AgentEvent::ToolEnd {
                call_id,
                name,
                result,
                is_error,
                raw_result,
            } => {
                assert_eq!(call_id, Some("c1".into()));
                assert_eq!(name, "bash");
                assert_eq!(result, "ok");
                assert!(!is_error);
                assert_eq!(raw_result, Some("full output".into()));
            }
            _ => panic!("expected ToolEnd"),
        }
    }

    #[test]
    fn test_finish() {
        let ev = dispatch(StreamEvent::<Value>::Finish);
        assert!(matches!(ev, AgentEvent::Completed));
    }

    #[test]
    fn test_provider_error() {
        let ev = dispatch(StreamEvent::<Value>::ProviderError {
            message: "rate limited".into(),
        });
        assert!(matches!(ev, AgentEvent::Error(s) if s == "rate limited"));
    }

    #[test]
    fn test_tool_error() {
        let ev = dispatch(StreamEvent::<Value>::ToolError {
            call_id: Some("c1".into()),
            error: "command failed".into(),
        });
        assert!(matches!(ev, AgentEvent::Error(s) if s == "command failed"));
    }

    #[test]
    fn test_irrelevant_events_are_dropped() {
        // Events that should NOT produce an AgentEvent
        assert_dropped(StreamEvent::<Value>::TaskStart {
            node_id: "think".into(),
            namespace: None,
        });
        assert_dropped(StreamEvent::<Value>::TaskEnd {
            node_id: "think".into(),
            result: Ok(()),
            namespace: None,
        });
        assert_dropped(StreamEvent::<Value>::Updates {
            node_id: "think".into(),
            state: json!({}),
            namespace: None,
        });
        assert_dropped(StreamEvent::<Value>::Values(json!({})));
        assert_dropped(StreamEvent::<Value>::Custom(json!({"key": "val"})));
        assert_dropped(StreamEvent::<Value>::TurnStart);
        assert_dropped(StreamEvent::<Value>::TurnFinish {
            reason: "stop".into(),
            usage: stream_event::Usage {
                input: 10,
                output: 20,
                reasoning: None,
                cache_read: None,
                cache_write: None,
            },
        });
        assert_dropped(StreamEvent::<Value>::Checkpoint(
            stream_event::CheckpointEvent {
                checkpoint_id: "cp-1".into(),
                timestamp: "now".into(),
                step: 0,
                state: json!({}),
                thread_id: None,
                checkpoint_ns: None,
            },
        ));
    }

    #[test]
    fn test_create_agent_channel_capacity() {
        let (tx, _rx) = create_agent_channel();
        // Channel capacity should be 256
        assert_eq!(tx.max_capacity(), 256);
    }

    #[test]
    fn test_channel_full_does_not_panic() {
        // When the channel is full, try_send should silently drop the event
        // (not panic).
        let (tx, _rx) = mpsc::channel::<AgentEvent>(1);
        let mut cb = create_stream_callback(tx);

        // First event should succeed (channel has capacity 1)
        cb(StreamEvent::<Value>::Finish);

        // Second event should fail silently (channel full, try_send returns Err)
        // but the callback should not panic.
        cb(StreamEvent::<Value>::Finish);
    }

    #[test]
    fn test_tool_call_none_call_id() {
        let ev = dispatch(StreamEvent::ToolCall {
            call_id: None,
            name: "read".into(),
            arguments: json!({"path": "file.txt"}),
        });
        match ev {
            AgentEvent::ToolCall { call_id, name, .. } => {
                assert!(call_id.is_none());
                assert_eq!(name, "read");
            }
            _ => panic!("expected ToolCall"),
        }
    }

    // -------------------------------------------------------------------------
    // Boundary conditions — empty strings
    // -------------------------------------------------------------------------

    #[test]
    fn test_empty_text_delta() {
        let ev = dispatch(StreamEvent::TextDelta {
            content: String::new(),
            metadata: Default::default(),
        });
        assert!(matches!(ev, AgentEvent::TextDelta(s) if s.is_empty()));
    }

    #[test]
    fn test_empty_reasoning_delta() {
        let ev = dispatch(StreamEvent::ReasoningDelta {
            id: "r1".into(),
            content: String::new(),
            metadata: Default::default(),
        });
        assert!(matches!(ev, AgentEvent::ReasoningDelta(s) if s.is_empty()));
    }

    #[test]
    fn test_empty_tool_output() {
        let ev = dispatch(StreamEvent::ToolOutput {
            call_id: Some("c1".into()),
            name: "cat".into(),
            content: String::new(),
        });
        match ev {
            AgentEvent::ToolOutput { content, .. } => assert!(content.is_empty()),
            _ => panic!("expected ToolOutput"),
        }
    }

    #[test]
    fn test_empty_tool_end_result() {
        let ev = dispatch(StreamEvent::ToolEnd {
            call_id: Some("c1".into()),
            name: "cat".into(),
            result: String::new(),
            is_error: false,
            raw_result: Some(String::new()),
        });
        match ev {
            AgentEvent::ToolEnd { result, .. } => assert!(result.is_empty()),
            _ => panic!("expected ToolEnd"),
        }
    }

    #[test]
    fn test_empty_error() {
        let ev = dispatch(StreamEvent::<Value>::ToolError {
            call_id: Some("c1".into()),
            error: String::new(),
        });
        assert!(matches!(ev, AgentEvent::Error(s) if s.is_empty()));
    }

    #[test]
    fn test_empty_provider_error() {
        let ev = dispatch(StreamEvent::<Value>::ProviderError {
            message: String::new(),
        });
        assert!(matches!(ev, AgentEvent::Error(s) if s.is_empty()));
    }

    // -------------------------------------------------------------------------
    // Boundary conditions — None / empty call_id for all tool variants
    // -------------------------------------------------------------------------

    #[test]
    fn test_tool_start_none_call_id() {
        let ev = dispatch(StreamEvent::ToolStart {
            call_id: None,
            name: "bash".into(),
        });
        match ev {
            AgentEvent::ToolStart { call_id, name } => {
                assert!(call_id.is_none());
                assert_eq!(name, "bash");
            }
            _ => panic!("expected ToolStart"),
        }
    }

    #[test]
    fn test_tool_output_none_call_id() {
        let ev = dispatch(StreamEvent::ToolOutput {
            call_id: None,
            name: "bash".into(),
            content: "output".into(),
        });
        match ev {
            AgentEvent::ToolOutput { call_id, name, .. } => {
                assert!(call_id.is_none());
                assert_eq!(name, "bash");
            }
            _ => panic!("expected ToolOutput"),
        }
    }

    #[test]
    fn test_tool_end_none_call_id() {
        let ev = dispatch(StreamEvent::ToolEnd {
            call_id: None,
            name: "bash".into(),
            result: "done".into(),
            is_error: false,
            raw_result: None,
        });
        match ev {
            AgentEvent::ToolEnd { call_id, name, .. } => {
                assert!(call_id.is_none());
                assert_eq!(name, "bash");
            }
            _ => panic!("expected ToolEnd"),
        }
    }

    #[test]
    fn test_tool_start_empty_call_id() {
        let ev = dispatch(StreamEvent::ToolStart {
            call_id: Some(String::new()),
            name: "read".into(),
        });
        match ev {
            AgentEvent::ToolStart { call_id, name } => {
                assert_eq!(call_id, Some(String::new()));
                assert_eq!(name, "read");
            }
            _ => panic!("expected ToolStart"),
        }
    }

    #[test]
    fn test_tool_output_empty_call_id() {
        let ev = dispatch(StreamEvent::ToolOutput {
            call_id: Some(String::new()),
            name: "read".into(),
            content: "data".into(),
        });
        match ev {
            AgentEvent::ToolOutput { call_id, .. } => {
                assert_eq!(call_id, Some(String::new()));
            }
            _ => panic!("expected ToolOutput"),
        }
    }

    #[test]
    fn test_tool_end_empty_call_id() {
        let ev = dispatch(StreamEvent::ToolEnd {
            call_id: Some(String::new()),
            name: "read".into(),
            result: "ok".into(),
            is_error: false,
            raw_result: None,
        });
        match ev {
            AgentEvent::ToolEnd { call_id, .. } => {
                assert_eq!(call_id, Some(String::new()));
            }
            _ => panic!("expected ToolEnd"),
        }
    }

    // -------------------------------------------------------------------------
    // Error paths
    // -------------------------------------------------------------------------

    #[test]
    fn test_tool_end_with_error() {
        let ev = dispatch(StreamEvent::ToolEnd {
            call_id: Some("c1".into()),
            name: "bash".into(),
            result: "exit code 1".into(),
            is_error: true,
            raw_result: None,
        });
        match ev {
            AgentEvent::ToolEnd {
                result,
                is_error,
                raw_result,
                ..
            } => {
                assert_eq!(result, "exit code 1");
                assert!(is_error);
                assert!(raw_result.is_none());
            }
            _ => panic!("expected ToolEnd"),
        }
    }

    #[test]
    fn test_tool_error_none_call_id() {
        let ev = dispatch(StreamEvent::<Value>::ToolError {
            call_id: None,
            error: "command failed".into(),
        });
        assert!(matches!(ev, AgentEvent::Error(s) if s == "command failed"));
    }

    #[test]
    fn test_tool_error_empty_call_id() {
        let ev = dispatch(StreamEvent::<Value>::ToolError {
            call_id: Some(String::new()),
            error: "timeout".into(),
        });
        assert!(matches!(ev, AgentEvent::Error(s) if s == "timeout"));
    }

    // -------------------------------------------------------------------------
    // Large payloads
    // -------------------------------------------------------------------------

    #[test]
    fn test_large_text_delta() {
        let large: String = (0..10_000).map(|_| 'a').collect();
        let ev = dispatch(StreamEvent::TextDelta {
            content: large.clone(),
            metadata: Default::default(),
        });
        match ev {
            AgentEvent::TextDelta(s) => {
                assert_eq!(s.len(), 10_000);
                assert_eq!(s, large);
            }
            _ => panic!("expected TextDelta"),
        }
    }

    #[test]
    fn test_large_json_arguments() {
        let nested = json!({
            "deep": {
                "arr": (0..100).map(|i| json!({"idx": i, "val": format!("x{}", i)})).collect::<Vec<_>>()
            }
        });
        let ev = dispatch(StreamEvent::ToolCall {
            call_id: Some("c1".into()),
            name: "process".into(),
            arguments: nested.clone(),
        });
        match ev {
            AgentEvent::ToolCall { arguments, .. } => {
                assert_eq!(arguments["deep"]["arr"].as_array().unwrap().len(), 100);
            }
            _ => panic!("expected ToolCall"),
        }
    }

    // -------------------------------------------------------------------------
    // Stream simulation — ordering and sequencing
    // -------------------------------------------------------------------------

    #[test]
    fn test_event_sequence_ordering() {
        let (tx, mut rx) = create_agent_channel();
        let mut cb = create_stream_callback(tx);

        cb(StreamEvent::TextDelta {
            content: "Hello ".into(),
            metadata: Default::default(),
        });
        cb(StreamEvent::TextDelta {
            content: "World".into(),
            metadata: Default::default(),
        });
        cb(StreamEvent::<Value>::Finish);

        let ev1 = rx.try_recv().expect("first event");
        assert!(matches!(ev1, AgentEvent::TextDelta(s) if s == "Hello "));

        let ev2 = rx.try_recv().expect("second event");
        assert!(matches!(ev2, AgentEvent::TextDelta(s) if s == "World"));

        let ev3 = rx.try_recv().expect("third event");
        assert!(matches!(ev3, AgentEvent::Completed));

        // No more events.
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_mixed_event_sequence() {
        let (tx, mut rx) = create_agent_channel();
        let mut cb = create_stream_callback(tx);

        cb(StreamEvent::ReasoningDelta {
            id: "r1".into(),
            content: "thinking...".into(),
            metadata: Default::default(),
        });
        cb(StreamEvent::ToolCall {
            call_id: Some("c1".into()),
            name: "bash".into(),
            arguments: json!({"cmd": "ls"}),
        });
        cb(StreamEvent::ToolStart {
            call_id: Some("c1".into()),
            name: "bash".into(),
        });
        cb(StreamEvent::ToolOutput {
            call_id: Some("c1".into()),
            name: "bash".into(),
            content: "file.txt".into(),
        });
        cb(StreamEvent::ToolEnd {
            call_id: Some("c1".into()),
            name: "bash".into(),
            result: "ok".into(),
            is_error: false,
            raw_result: None,
        });
        cb(StreamEvent::<Value>::Finish);

        assert!(matches!(
            rx.try_recv().unwrap(),
            AgentEvent::ReasoningDelta(_)
        ));
        assert!(matches!(rx.try_recv().unwrap(), AgentEvent::ToolCall { .. }));
        assert!(matches!(rx.try_recv().unwrap(), AgentEvent::ToolStart { .. }));
        assert!(matches!(rx.try_recv().unwrap(), AgentEvent::ToolOutput { .. }));
        assert!(matches!(rx.try_recv().unwrap(), AgentEvent::ToolEnd { .. }));
        assert!(matches!(rx.try_recv().unwrap(), AgentEvent::Completed));
        assert!(rx.try_recv().is_err());
    }

    // -------------------------------------------------------------------------
    // Unicode / multibyte strings
    // -------------------------------------------------------------------------

    #[test]
    fn test_unicode_text_delta() {
        let ev = dispatch(StreamEvent::TextDelta {
            content: "你好，世界 🌍🚀".into(),
            metadata: Default::default(),
        });
        assert!(matches!(ev, AgentEvent::TextDelta(s) if s == "你好，世界 🌍🚀"));
    }

    #[test]
    fn test_unicode_reasoning_delta() {
        let ev = dispatch(StreamEvent::ReasoningDelta {
            id: "r1".into(),
            content: "思考中... 🔍".into(),
            metadata: Default::default(),
        });
        assert!(matches!(ev, AgentEvent::ReasoningDelta(s) if s == "思考中... 🔍"));
    }

    // -------------------------------------------------------------------------
    // Channel boundary — dropped receiver
    // -------------------------------------------------------------------------

    #[test]
    fn test_receiver_dropped_no_panic() {
        let (tx, rx) = create_agent_channel();
        drop(rx); // Drop the receiver so try_send returns Err(Closed)

        let mut cb = create_stream_callback(tx);
        // Must not panic when channel is closed
        cb(StreamEvent::<Value>::Finish);
        cb(StreamEvent::TextDelta {
            content: "hello".into(),
            metadata: Default::default(),
        });
        cb(StreamEvent::<Value>::ProviderError {
            message: "err".into(),
        });
    }

    // -------------------------------------------------------------------------
    // Debug / Clone trait verification
    // -------------------------------------------------------------------------

    #[test]
    fn test_agent_event_debug() {
        let ev = AgentEvent::TextDelta("hello".into());
        let s = format!("{:?}", ev);
        assert!(s.contains("TextDelta"));

        let ev = AgentEvent::ToolCall {
            call_id: Some("c1".into()),
            name: "bash".into(),
            arguments: json!({"cmd": "ls"}),
        };
        let s = format!("{:?}", ev);
        assert!(s.contains("ToolCall"));
        assert!(s.contains("bash"));

        let ev = AgentEvent::Completed;
        let s = format!("{:?}", ev);
        assert!(s.contains("Completed"));
    }

    #[test]
    fn test_agent_event_clone() {
        let ev = AgentEvent::ToolEnd {
            call_id: Some("c1".into()),
            name: "bash".into(),
            result: "ok".into(),
            is_error: true,
            raw_result: Some("full".into()),
        };
        let cloned = ev.clone();
        match cloned {
            AgentEvent::ToolEnd {
                call_id,
                name,
                result,
                is_error,
                raw_result,
            } => {
                assert_eq!(call_id, Some("c1".into()));
                assert_eq!(name, "bash");
                assert_eq!(result, "ok");
                assert!(is_error);
                assert_eq!(raw_result, Some("full".into()));
            }
            _ => panic!("expected ToolEnd"),
        }
    }

    // -------------------------------------------------------------------------
    // JSON arguments boundary — various shapes
    // -------------------------------------------------------------------------

    #[test]
    fn test_tool_call_json_null_arguments() {
        let ev = dispatch(StreamEvent::ToolCall {
            call_id: Some("c1".into()),
            name: "noop".into(),
            arguments: Value::Null,
        });
        match ev {
            AgentEvent::ToolCall { arguments, .. } => {
                assert!(arguments.is_null());
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn test_tool_call_json_array_arguments() {
        let ev = dispatch(StreamEvent::ToolCall {
            call_id: None,
            name: "batch".into(),
            arguments: json!([1, 2, 3]),
        });
        match ev {
            AgentEvent::ToolCall { arguments, .. } => {
                assert_eq!(arguments.as_array().unwrap().len(), 3);
            }
            _ => panic!("expected ToolCall"),
        }
    }
}