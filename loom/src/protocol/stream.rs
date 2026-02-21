//! Streaming output protocol (protocol_spec).
//!
//! Event serialization as **type + payload** per [protocol_spec](https://github.com/loom/loom/blob/main/docs/protocol_spec.md) §4,
//! and optional **envelope** (session_id, node_id, event_id) per §2 / §7.1.
//!
//! [`Envelope`] and [`EnvelopeState`] are defined in the `stream-event` crate; loom re-exports them
//! and provides the bridge from [`StreamEvent<S>`](crate::stream::StreamEvent) to [`ProtocolEvent`](stream_event::ProtocolEvent).

pub use stream_event::{to_json as stream_event_to_json, Envelope, ProtocolEvent};

use super::ProtocolEventEnvelope;
use crate::stream::{MessageChunk, StreamEvent, StreamMetadata};
use serde::Serialize;
use serde_json::{json, Value};
use std::fmt::Debug;

/// Converts a `StreamEvent<S>` into a `ProtocolEvent` (state-carrying variants serialize `S` to `Value`).
/// Callers then use [`stream_event::to_json`] with [`EnvelopeState`](crate::protocol::EnvelopeState) to produce the final JSON.
pub fn stream_event_to_protocol_event<S>(
    ev: &StreamEvent<S>,
) -> Result<ProtocolEvent, serde_json::Error>
where
    S: Serialize + Clone + Send + Sync + Debug + 'static,
{
    let pe = match ev {
        StreamEvent::TaskStart { node_id } => ProtocolEvent::NodeEnter {
            id: node_id.clone(),
        },
        StreamEvent::TaskEnd { node_id, result } => {
            let result_json = match result {
                Ok(()) => json!("Ok"),
                Err(e) => json!({ "Err": e }),
            };
            ProtocolEvent::NodeExit {
                id: node_id.clone(),
                result: result_json,
            }
        }
        StreamEvent::Messages {
            chunk: MessageChunk { content },
            metadata: StreamMetadata { loom_node },
        } => ProtocolEvent::MessageChunk {
            content: content.clone(),
            id: loom_node.clone(),
        },
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        } => ProtocolEvent::Usage {
            prompt_tokens: *prompt_tokens,
            completion_tokens: *completion_tokens,
            total_tokens: *total_tokens,
        },
        StreamEvent::Values(state) => ProtocolEvent::Values {
            state: serde_json::to_value(state)?,
        },
        StreamEvent::Updates { node_id, state } => ProtocolEvent::Updates {
            id: node_id.clone(),
            state: serde_json::to_value(state)?,
        },
        StreamEvent::Custom(v) => ProtocolEvent::Custom { value: v.clone() },
        StreamEvent::Checkpoint(cp) => ProtocolEvent::Checkpoint {
            checkpoint_id: cp.checkpoint_id.clone(),
            timestamp: cp.timestamp.clone(),
            step: cp.step,
            state: serde_json::to_value(&cp.state)?,
            thread_id: cp.thread_id.clone(),
            checkpoint_ns: cp.checkpoint_ns.clone(),
        },
        StreamEvent::TotExpand { candidates } => ProtocolEvent::TotExpand {
            candidates: candidates.clone(),
        },
        StreamEvent::TotEvaluate { chosen, scores } => ProtocolEvent::TotEvaluate {
            chosen: *chosen,
            scores: scores.clone(),
        },
        StreamEvent::TotBacktrack { reason, to_depth } => ProtocolEvent::TotBacktrack {
            reason: reason.clone(),
            to_depth: *to_depth,
        },
        StreamEvent::GotPlan {
            node_count,
            edge_count,
            node_ids,
        } => ProtocolEvent::GotPlan {
            node_count: *node_count,
            edge_count: *edge_count,
            node_ids: node_ids.clone(),
        },
        StreamEvent::GotNodeStart { node_id } => ProtocolEvent::GotNodeStart {
            id: node_id.clone(),
        },
        StreamEvent::GotNodeComplete {
            node_id,
            result_summary,
        } => ProtocolEvent::GotNodeComplete {
            id: node_id.clone(),
            result_summary: result_summary.clone(),
        },
        StreamEvent::GotNodeFailed { node_id, error } => ProtocolEvent::GotNodeFailed {
            id: node_id.clone(),
            error: error.clone(),
        },
        StreamEvent::GotExpand {
            node_id,
            nodes_added,
            edges_added,
        } => ProtocolEvent::GotExpand {
            node_id: node_id.clone(),
            nodes_added: *nodes_added,
            edges_added: *edges_added,
        },
        StreamEvent::ToolCallChunk {
            call_id,
            name,
            arguments_delta,
        } => ProtocolEvent::ToolCallChunk {
            call_id: call_id.clone(),
            name: name.clone(),
            arguments_delta: arguments_delta.clone(),
        },
        StreamEvent::ToolCall {
            call_id,
            name,
            arguments,
        } => ProtocolEvent::ToolCall {
            call_id: call_id.clone(),
            name: name.clone(),
            arguments: arguments.clone(),
        },
        StreamEvent::ToolStart { call_id, name } => ProtocolEvent::ToolStart {
            call_id: call_id.clone(),
            name: name.clone(),
        },
        StreamEvent::ToolOutput {
            call_id,
            name,
            content,
        } => ProtocolEvent::ToolOutput {
            call_id: call_id.clone(),
            name: name.clone(),
            content: content.clone(),
        },
        StreamEvent::ToolEnd {
            call_id,
            name,
            result,
            is_error,
        } => ProtocolEvent::ToolEnd {
            call_id: call_id.clone(),
            name: name.clone(),
            result: result.clone(),
            is_error: *is_error,
        },
        StreamEvent::ToolApproval {
            call_id,
            name,
            arguments,
        } => ProtocolEvent::ToolApproval {
            call_id: call_id.clone(),
            name: name.clone(),
            arguments: arguments.clone(),
        },
    };
    Ok(pe)
}

/// Converts a `StreamEvent<S>` to a typed protocol event with envelope injected
/// (`session_id`, `node_id`, `event_id`).
pub fn stream_event_to_protocol_envelope<S>(
    ev: &StreamEvent<S>,
    state: &mut stream_event::EnvelopeState,
) -> Result<ProtocolEventEnvelope, serde_json::Error>
where
    S: Serialize + Clone + Send + Sync + Debug + 'static,
{
    let protocol_ev = stream_event_to_protocol_event(ev)?;
    let value = stream_event::to_json(&protocol_ev, state)?;
    ProtocolEventEnvelope::from_value(value)
}

/// Converts a `StreamEvent<S>` to protocol JSON with envelope injected (session_id, node_id, event_id).
/// This is the main API for JSON-producing callers.
pub fn stream_event_to_protocol_value<S>(
    ev: &StreamEvent<S>,
    state: &mut stream_event::EnvelopeState,
) -> Result<Value, serde_json::Error>
where
    S: Serialize + Clone + Send + Sync + Debug + 'static,
{
    let event = stream_event_to_protocol_envelope(ev, state)?;
    event.to_value()
}

/// Converts a `StreamEvent<S>` to protocol format: top-level **type** + payload (protocol_spec §4.2), **without** envelope.
/// Prefer [`stream_event_to_protocol_value`] when you have [`EnvelopeState`](crate::protocol::EnvelopeState) and want envelope injected.
pub fn stream_event_to_protocol_format<S>(ev: &StreamEvent<S>) -> Result<Value, serde_json::Error>
where
    S: Serialize + Clone + Send + Sync + Debug + 'static,
{
    let protocol_ev = stream_event_to_protocol_event(ev)?;
    protocol_ev.to_value()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::StreamMetadata;

    #[derive(Clone, Debug, serde::Serialize)]
    struct DummyState(i32);

    #[test]
    fn node_enter_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TaskStart {
            node_id: "think".to_string(),
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "node_enter");
        assert_eq!(v["id"], "think");
    }

    #[test]
    fn node_exit_ok_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TaskEnd {
            node_id: "act".to_string(),
            result: Ok(()),
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "node_exit");
        assert_eq!(v["id"], "act");
        assert_eq!(v["result"], "Ok");
    }

    #[test]
    fn message_chunk_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Messages {
            chunk: MessageChunk {
                content: "hello".to_string(),
            },
            metadata: StreamMetadata {
                loom_node: "think".to_string(),
            },
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "message_chunk");
        assert_eq!(v["content"], "hello");
        assert_eq!(v["id"], "think");
    }

    #[test]
    fn usage_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "usage");
        assert_eq!(v["prompt_tokens"], 10);
        assert_eq!(v["total_tokens"], 15);
    }

    #[test]
    fn values_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Values(DummyState(42));
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "values");
        assert_eq!(v["state"], 42);
    }

    #[test]
    fn updates_format_uses_payload_id() {
        let ev: StreamEvent<DummyState> = StreamEvent::Updates {
            node_id: "think".to_string(),
            state: DummyState(7),
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "updates");
        assert_eq!(v["id"], "think");
        assert_eq!(v["state"], 7);
        assert!(v.get("node_id").is_none());
    }

    #[test]
    fn node_exit_err_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TaskEnd {
            node_id: "fail".to_string(),
            result: Err("boom".to_string()),
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "node_exit");
        assert_eq!(v["id"], "fail");
        assert_eq!(v["result"]["Err"], "boom");
    }

    #[test]
    fn stream_event_to_protocol_value_injects_envelope() {
        let mut state = crate::protocol::EnvelopeState::new("sess-1".to_string());
        let enter: StreamEvent<DummyState> = StreamEvent::TaskStart {
            node_id: "think".to_string(),
        };
        let usage: StreamEvent<DummyState> = StreamEvent::Usage {
            prompt_tokens: 1,
            completion_tokens: 2,
            total_tokens: 3,
        };

        let first = stream_event_to_protocol_value(&enter, &mut state).unwrap();
        let second = stream_event_to_protocol_value(&usage, &mut state).unwrap();

        assert_eq!(first["type"], "node_enter");
        assert_eq!(first["session_id"], "sess-1");
        assert_eq!(first["node_id"], "run-think-0");
        assert_eq!(first["event_id"], 1);

        assert_eq!(second["type"], "usage");
        assert_eq!(second["session_id"], "sess-1");
        assert_eq!(second["node_id"], "run-think-0");
        assert_eq!(second["event_id"], 2);
    }

    #[test]
    fn stream_event_to_protocol_envelope_is_typed() {
        let mut state = crate::protocol::EnvelopeState::new("sess-1".to_string());
        let enter: StreamEvent<DummyState> = StreamEvent::TaskStart {
            node_id: "think".to_string(),
        };

        let event = stream_event_to_protocol_envelope(&enter, &mut state).unwrap();

        assert_eq!(event.session_id.as_deref(), Some("sess-1"));
        assert_eq!(event.node_id.as_deref(), Some("run-think-0"));
        assert_eq!(event.event_id, Some(1));
        match event.event {
            ProtocolEvent::NodeEnter { id } => assert_eq!(id, "think"),
            _ => panic!("expected node_enter"),
        }
    }

    #[test]
    fn tool_call_chunk_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolCallChunk {
            call_id: Some("c1".into()),
            name: Some("bash".into()),
            arguments_delta: "{\"cmd\":".into(),
        };
        let v = stream_event_to_protocol_event(&ev)
            .unwrap()
            .to_value()
            .unwrap();
        assert_eq!(v["type"], "tool_call_chunk");
        assert_eq!(v["call_id"], "c1");
        assert_eq!(v["name"], "bash");
        assert_eq!(v["arguments_delta"], "{\"cmd\":");
    }

    #[test]
    fn tool_call_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolCall {
            call_id: Some("c1".into()),
            name: "list_dir".into(),
            arguments: serde_json::json!({"path": "."}),
        };
        let v = stream_event_to_protocol_event(&ev)
            .unwrap()
            .to_value()
            .unwrap();
        assert_eq!(v["type"], "tool_call");
        assert_eq!(v["name"], "list_dir");
        assert_eq!(v["arguments"]["path"], ".");
    }

    #[test]
    fn tool_start_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolStart {
            call_id: Some("c1".into()),
            name: "bash".into(),
        };
        let v = stream_event_to_protocol_event(&ev)
            .unwrap()
            .to_value()
            .unwrap();
        assert_eq!(v["type"], "tool_start");
        assert_eq!(v["name"], "bash");
    }

    #[test]
    fn tool_output_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolOutput {
            call_id: Some("c1".into()),
            name: "bash".into(),
            content: "hello\n".into(),
        };
        let v = stream_event_to_protocol_event(&ev)
            .unwrap()
            .to_value()
            .unwrap();
        assert_eq!(v["type"], "tool_output");
        assert_eq!(v["content"], "hello\n");
    }

    #[test]
    fn tool_end_success_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolEnd {
            call_id: Some("c1".into()),
            name: "bash".into(),
            result: "done".into(),
            is_error: false,
        };
        let v = stream_event_to_protocol_event(&ev)
            .unwrap()
            .to_value()
            .unwrap();
        assert_eq!(v["type"], "tool_end");
        assert_eq!(v["result"], "done");
        assert_eq!(v["is_error"], false);
    }

    #[test]
    fn tool_end_error_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolEnd {
            call_id: Some("c1".into()),
            name: "bash".into(),
            result: "Error: fail".into(),
            is_error: true,
        };
        let v = stream_event_to_protocol_event(&ev)
            .unwrap()
            .to_value()
            .unwrap();
        assert_eq!(v["type"], "tool_end");
        assert_eq!(v["is_error"], true);
    }

    #[test]
    fn tool_approval_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolApproval {
            call_id: Some("c2".into()),
            name: "delete_file".into(),
            arguments: serde_json::json!({"path": "x.txt"}),
        };
        let v = stream_event_to_protocol_event(&ev)
            .unwrap()
            .to_value()
            .unwrap();
        assert_eq!(v["type"], "tool_approval");
        assert_eq!(v["name"], "delete_file");
        assert_eq!(v["arguments"]["path"], "x.txt");
    }
}
