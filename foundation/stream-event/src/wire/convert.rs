//! Stream event conversion: StreamEvent → ProtocolEvent / Format A JSON.
//!
//! Migrated from anureo-protocol crate (responses.rs + stream.rs + export.rs).

use crate::types::metadata::StreamMetadata;
use crate::types::stream_event::StreamEvent;
use crate::wire::envelope::{to_json as stream_event_to_json, EnvelopeState};
use crate::wire::protocol::ProtocolEvent;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt::Debug;

// ---------------------------------------------------------------------------
// ProtocolEventEnvelope (from responses.rs)
// ---------------------------------------------------------------------------

/// Typed protocol stream event payload with optional envelope fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProtocolEventEnvelope {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<u64>,
    #[serde(flatten)]
    pub event: ProtocolEvent,
}

impl ProtocolEventEnvelope {
    /// Serializes the typed event envelope into a JSON object.
    pub fn to_value(&self) -> Result<Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    /// Deserializes a JSON object into a typed event envelope.
    pub fn from_value(value: Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value)
    }
}

// ---------------------------------------------------------------------------
// StreamEvent → ProtocolEvent (from stream.rs)
// ---------------------------------------------------------------------------

/// Converts a `StreamEvent<S>` into a `ProtocolEvent` (state-carrying variants serialize `S` to `Value`).
pub fn stream_event_to_protocol_event<S>(
    ev: &StreamEvent<S>,
) -> Result<ProtocolEvent, serde_json::Error>
where
    S: Serialize + Clone + Send + Sync + Debug + 'static,
{
    let pe = match ev {
        StreamEvent::TaskStart { node_id, .. } => ProtocolEvent::NodeEnter {
            id: node_id.clone(),
        },
        StreamEvent::TaskEnd {
            node_id, result, ..
        } => {
            let result_json = match result {
                Ok(()) => json!("Ok"),
                Err(e) => json!({ "Err": e }),
            };
            ProtocolEvent::NodeExit {
                id: node_id.clone(),
                result: result_json,
            }
        }
        StreamEvent::TextDelta {
            content,
            metadata: StreamMetadata { anureo_node, .. },
        } => ProtocolEvent::TextDelta {
            content: content.clone(),
            id: anureo_node.clone(),
        },
        StreamEvent::ReasoningDelta {
            id,
            content,
            metadata: StreamMetadata { anureo_node, .. },
        } => ProtocolEvent::ReasoningDelta {
            reasoning_id: id.clone(),
            content: content.clone(),
            id: anureo_node.clone(),
        },
        StreamEvent::TurnFinish { usage, .. } => ProtocolEvent::Usage {
            input: usage.input,
            output: usage.output,
        },
        StreamEvent::Values(state) => ProtocolEvent::Values {
            state: serde_json::to_value(state)?,
        },
        StreamEvent::Updates { node_id, state, .. } => ProtocolEvent::Updates {
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
        StreamEvent::ToolInputStart { call_id, name } => ProtocolEvent::Custom {
            value: json!({ "type": "tool_input_start", "call_id": call_id, "name": name }),
        },
        StreamEvent::ToolInputDelta {
            call_id,
            arguments_delta,
        } => ProtocolEvent::Custom {
            value: json!({ "type": "tool_input_delta", "call_id": call_id, "arguments_delta": arguments_delta }),
        },
        StreamEvent::ToolInputEnd { call_id, arguments } => ProtocolEvent::Custom {
            value: json!({ "type": "tool_input_end", "call_id": call_id, "arguments": arguments }),
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
            raw_result,
        } => ProtocolEvent::ToolEnd {
            call_id: call_id.clone(),
            name: name.clone(),
            result: result.clone(),
            is_error: *is_error,
            raw_result: raw_result.clone(),
        },
        StreamEvent::TextBlockStart { metadata } => ProtocolEvent::Custom {
            value: json!({ "type": "text_block_start", "metadata": metadata }),
        },
        StreamEvent::TextBlockEnd { metadata } => ProtocolEvent::Custom {
            value: json!({ "type": "text_block_end", "metadata": metadata }),
        },
        StreamEvent::ReasoningBlockStart { id, metadata } => ProtocolEvent::Custom {
            value: json!({ "type": "reasoning_block_start", "id": id, "metadata": metadata }),
        },
        StreamEvent::ReasoningBlockEnd { id, metadata } => ProtocolEvent::Custom {
            value: json!({ "type": "reasoning_block_end", "id": id, "metadata": metadata }),
        },
        StreamEvent::TurnStart => ProtocolEvent::Custom {
            value: json!({ "type": "turn_start" }),
        },
        StreamEvent::ToolError { call_id, error } => ProtocolEvent::Custom {
            value: json!({ "type": "tool_error", "call_id": call_id, "error": error }),
        },
        StreamEvent::ProviderError { message } => ProtocolEvent::Custom {
            value: json!({ "type": "provider_error", "message": message }),
        },
        StreamEvent::Finish => ProtocolEvent::Custom {
            value: json!({ "type": "finish" }),
        },
    };
    Ok(pe)
}

/// Converts a `StreamEvent<S>` to a typed protocol event with envelope injected
/// (`session_id`, `node_id`, `event_id`).
pub fn stream_event_to_protocol_envelope<S>(
    ev: &StreamEvent<S>,
    state: &mut EnvelopeState,
) -> Result<ProtocolEventEnvelope, serde_json::Error>
where
    S: Serialize + Clone + Send + Sync + Debug + 'static,
{
    let protocol_ev = stream_event_to_protocol_event(ev)?;
    let value = stream_event_to_json(&protocol_ev, state)?;
    ProtocolEventEnvelope::from_value(value)
}

// ---------------------------------------------------------------------------
// StreamEvent → Format A JSON (from export.rs)
// ---------------------------------------------------------------------------

/// Converts a `StreamEvent<S>` to format A JSON (single-key object, externally tagged).
///
/// Output shape: `{"TaskStart":{"node_id":"think"}}`, `{"Usage":{...}}`, etc.
pub fn stream_event_to_format_a<S>(ev: &StreamEvent<S>) -> Result<Value, serde_json::Error>
where
    S: Serialize + Clone + Send + Sync + Debug + 'static,
{
    let obj = match ev {
        StreamEvent::Values(state) => {
            let state_json = serde_json::to_value(state)?;
            json!({ "Values": state_json })
        }
        StreamEvent::Updates {
            node_id,
            state,
            namespace,
        } => {
            let state_json = serde_json::to_value(state)?;
            json!({ "Updates": { "node_id": node_id, "state": state_json, "namespace": namespace } })
        }
        StreamEvent::TextDelta { content, metadata } => json!({
            "TextDelta": { "content": content, "metadata": metadata }
        }),
        StreamEvent::ReasoningDelta {
            id,
            content,
            metadata,
        } => json!({
            "ReasoningDelta": { "id": id, "content": content, "metadata": metadata }
        }),
        StreamEvent::Custom(v) => json!({ "Custom": v }),
        StreamEvent::Checkpoint(cp) => {
            let state_json = serde_json::to_value(&cp.state)?;
            json!({
                "Checkpoint": {
                    "checkpoint_id": cp.checkpoint_id,
                    "timestamp": cp.timestamp,
                    "step": cp.step,
                    "state": state_json,
                    "thread_id": cp.thread_id,
                    "checkpoint_ns": cp.checkpoint_ns
                }
            })
        }
        StreamEvent::TaskStart { node_id, namespace } => {
            json!({ "TaskStart": { "node_id": node_id, "namespace": namespace } })
        }
        StreamEvent::TaskEnd {
            node_id,
            result,
            namespace,
        } => {
            let result_json = match result {
                Ok(()) => json!("Ok"),
                Err(e) => json!({ "Err": e }),
            };
            json!({ "TaskEnd": { "node_id": node_id, "result": result_json, "namespace": namespace } })
        }
        StreamEvent::TotExpand { candidates } => {
            json!({ "TotExpand": { "candidates": candidates } })
        }
        StreamEvent::TotEvaluate { chosen, scores } => {
            json!({ "TotEvaluate": { "chosen": chosen, "scores": scores } })
        }
        StreamEvent::TotBacktrack { reason, to_depth } => {
            json!({ "TotBacktrack": { "reason": reason, "to_depth": to_depth } })
        }
        StreamEvent::GotPlan {
            node_count,
            edge_count,
            node_ids,
        } => json!({
            "GotPlan": { "node_count": node_count, "edge_count": edge_count, "node_ids": node_ids }
        }),
        StreamEvent::GotNodeStart { node_id } => json!({ "GotNodeStart": { "node_id": node_id } }),
        StreamEvent::GotNodeComplete {
            node_id,
            result_summary,
        } => json!({
            "GotNodeComplete": { "node_id": node_id, "result_summary": result_summary }
        }),
        StreamEvent::GotNodeFailed { node_id, error } => {
            json!({ "GotNodeFailed": { "node_id": node_id, "error": error } })
        }
        StreamEvent::GotExpand {
            node_id,
            nodes_added,
            edges_added,
        } => json!({
            "GotExpand": { "node_id": node_id, "nodes_added": nodes_added, "edges_added": edges_added }
        }),
        StreamEvent::TurnFinish { reason, usage } => json!({
            "TurnFinish": { "reason": reason, "usage": usage }
        }),
        StreamEvent::ToolInputStart { call_id, name } => json!({
            "ToolInputStart": { "call_id": call_id, "name": name }
        }),
        StreamEvent::ToolInputDelta {
            call_id,
            arguments_delta,
        } => json!({
            "ToolInputDelta": { "call_id": call_id, "arguments_delta": arguments_delta }
        }),
        StreamEvent::ToolInputEnd { call_id, arguments } => json!({
            "ToolInputEnd": { "call_id": call_id, "arguments": arguments }
        }),
        StreamEvent::ToolCall {
            call_id,
            name,
            arguments,
        } => json!({
            "ToolCall": { "call_id": call_id, "name": name, "arguments": arguments }
        }),
        StreamEvent::ToolStart { call_id, name } => json!({
            "ToolStart": { "call_id": call_id, "name": name }
        }),
        StreamEvent::ToolOutput {
            call_id,
            name,
            content,
        } => json!({
            "ToolOutput": { "call_id": call_id, "name": name, "content": content }
        }),
        StreamEvent::ToolEnd {
            call_id,
            name,
            result,
            is_error,
            raw_result,
        } => {
            let mut obj = json!({
                "ToolEnd": { "call_id": call_id, "name": name, "result": result, "is_error": is_error }
            });
            if let Some(rr) = raw_result {
                obj["ToolEnd"]["raw_result"] = json!(rr);
            }
            obj
        }
        StreamEvent::TextBlockStart { metadata } => {
            json!({ "TextBlockStart": { "metadata": metadata } })
        }
        StreamEvent::TextBlockEnd { metadata } => {
            json!({ "TextBlockEnd": { "metadata": metadata } })
        }
        StreamEvent::ReasoningBlockStart { id, metadata } => {
            json!({ "ReasoningBlockStart": { "id": id, "metadata": metadata } })
        }
        StreamEvent::ReasoningBlockEnd { id, metadata } => {
            json!({ "ReasoningBlockEnd": { "id": id, "metadata": metadata } })
        }
        StreamEvent::TurnStart => json!({ "TurnStart": {} }),
        StreamEvent::ToolError { call_id, error } => {
            json!({ "ToolError": { "call_id": call_id, "error": error } })
        }
        StreamEvent::ProviderError { message } => {
            json!({ "ProviderError": { "message": message } })
        }
        StreamEvent::Finish => json!({ "Finish": {} }),
    };
    Ok(obj)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CheckpointEvent;

    #[derive(Clone, Debug, serde::Serialize)]
    struct DummyState(i32);

    fn to_value<S>(ev: &StreamEvent<S>, state: &mut EnvelopeState) -> Value
    where
        S: Serialize + Clone + Send + Sync + Debug + 'static,
    {
        stream_event_to_protocol_envelope(ev, state)
            .unwrap()
            .to_value()
            .unwrap()
    }

    // --- stream_event_to_protocol_event tests ---

    #[test]
    fn node_enter_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TaskStart {
            node_id: "think".to_string(),
            namespace: None,
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
            namespace: None,
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "node_exit");
        assert_eq!(v["id"], "act");
        assert_eq!(v["result"], "Ok");
    }

    #[test]
    fn text_delta_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TextDelta {
            content: "hello".to_string(),
            metadata: StreamMetadata {
                anureo_node: "think".to_string(),
                namespace: None,
            },
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "text_delta");
        assert_eq!(v["content"], "hello");
        assert_eq!(v["id"], "think");
    }

    #[test]
    fn reasoning_delta_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ReasoningDelta {
            id: "r0".to_string(),
            content: "reasoning step".to_string(),
            metadata: StreamMetadata {
                anureo_node: "think".to_string(),
                namespace: None,
            },
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "reasoning_delta");
        assert_eq!(v["content"], "reasoning step");
        assert_eq!(v["id"], "think");
    }

    #[test]
    fn protocol_usage_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TurnFinish {
            reason: "stop".to_string(),
            usage: crate::Usage {
                input: 10,
                output: 5,
                reasoning: None,
                cache_read: None,
                cache_write: None,
            },
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "usage");
        assert_eq!(v["input"], 10);
        assert_eq!(v["output"], 5);
    }

    #[test]
    fn protocol_values_format() {
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
            namespace: None,
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
            namespace: None,
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "node_exit");
        assert_eq!(v["id"], "fail");
        assert_eq!(v["result"]["Err"], "boom");
    }

    // --- envelope injection tests (via stream_event_to_protocol_envelope) ---

    #[test]
    fn envelope_injects_envelope() {
        let mut state = EnvelopeState::new("sess-1".to_string());
        let enter: StreamEvent<DummyState> = StreamEvent::TaskStart {
            node_id: "think".to_string(),
            namespace: None,
        };
        let usage: StreamEvent<DummyState> = StreamEvent::TurnFinish {
            reason: "stop".to_string(),
            usage: crate::Usage {
                input: 1,
                output: 2,
                reasoning: None,
                cache_read: None,
                cache_write: None,
            },
        };

        let first = to_value(&enter, &mut state);
        let second = to_value(&usage, &mut state);

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
    fn envelope_reasoning_delta_injects_envelope() {
        let mut state = EnvelopeState::new("sess-1".to_string());
        let enter: StreamEvent<DummyState> = StreamEvent::TaskStart {
            node_id: "think".to_string(),
            namespace: None,
        };
        let thought: StreamEvent<DummyState> = StreamEvent::ReasoningDelta {
            id: "r0".to_string(),
            content: "reasoning content".to_string(),
            metadata: StreamMetadata {
                anureo_node: "think".to_string(),
                namespace: None,
            },
        };

        let _ = to_value(&enter, &mut state);
        let v = to_value(&thought, &mut state);

        assert_eq!(v["type"], "reasoning_delta");
        assert_eq!(v["content"], "reasoning content");
        assert_eq!(v["id"], "think");
        assert_eq!(v["session_id"], "sess-1");
        assert_eq!(v["node_id"], "run-think-0");
        assert_eq!(v["event_id"], 2);
    }

    #[test]
    fn envelope_text_delta_injects_envelope() {
        let mut state = EnvelopeState::new("sess-1".to_string());
        let enter: StreamEvent<DummyState> = StreamEvent::TaskStart {
            node_id: "think".to_string(),
            namespace: None,
        };
        let msg: StreamEvent<DummyState> = StreamEvent::TextDelta {
            content: "final reply".to_string(),
            metadata: StreamMetadata {
                anureo_node: "think".to_string(),
                namespace: None,
            },
        };

        let _ = to_value(&enter, &mut state);
        let v = to_value(&msg, &mut state);

        assert_eq!(v["type"], "text_delta");
        assert_eq!(v["content"], "final reply");
        assert_eq!(v["id"], "think");
        assert_eq!(v["session_id"], "sess-1");
        assert_eq!(v["node_id"], "run-think-0");
    }

    #[test]
    fn envelope_is_typed() {
        let mut state = EnvelopeState::new("sess-1".to_string());
        let enter: StreamEvent<DummyState> = StreamEvent::TaskStart {
            node_id: "think".to_string(),
            namespace: None,
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

    // --- protocol event: tool variants ---

    #[test]
    fn protocol_tool_call_format() {
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
    fn protocol_tool_start_format() {
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
    fn protocol_tool_output_format() {
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
    fn protocol_tool_end_success_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolEnd {
            call_id: Some("c1".into()),
            name: "bash".into(),
            result: "done".into(),
            is_error: false,
            raw_result: None,
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
    fn protocol_tool_end_error_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolEnd {
            call_id: Some("c1".into()),
            name: "bash".into(),
            result: "Error: fail".into(),
            is_error: true,
            raw_result: None,
        };
        let v = stream_event_to_protocol_event(&ev)
            .unwrap()
            .to_value()
            .unwrap();
        assert_eq!(v["type"], "tool_end");
        assert_eq!(v["is_error"], true);
    }

    // --- protocol event: other variants ---

    #[test]
    fn protocol_custom_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Custom(json!({"key": "val"}));
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "custom");
        assert_eq!(v["value"]["key"], "val");
    }

    #[test]
    fn protocol_checkpoint_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Checkpoint(CheckpointEvent {
            checkpoint_id: "cp-1".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            step: 5,
            state: DummyState(99),
            thread_id: Some("t1".to_string()),
            checkpoint_ns: Some("ns".to_string()),
        });
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "checkpoint");
        assert_eq!(v["checkpoint_id"], "cp-1");
        assert_eq!(v["step"], 5);
    }

    #[test]
    fn protocol_tot_expand_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TotExpand {
            candidates: vec!["a".to_string(), "b".to_string()],
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "tot_expand");
        assert_eq!(v["candidates"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn protocol_tot_evaluate_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TotEvaluate {
            chosen: 1,
            scores: vec![0.5, 0.9],
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "tot_evaluate");
        assert_eq!(v["chosen"], 1);
    }

    #[test]
    fn protocol_tot_backtrack_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TotBacktrack {
            reason: "low score".to_string(),
            to_depth: 2,
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "tot_backtrack");
        assert_eq!(v["reason"], "low score");
        assert_eq!(v["to_depth"], 2);
    }

    #[test]
    fn protocol_got_plan_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotPlan {
            node_count: 3,
            edge_count: 2,
            node_ids: vec!["n1".to_string(), "n2".to_string(), "n3".to_string()],
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "got_plan");
        assert_eq!(v["node_count"], 3);
        assert_eq!(v["edge_count"], 2);
    }

    #[test]
    fn protocol_got_node_start_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotNodeStart {
            node_id: "gn1".to_string(),
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "got_node_start");
        assert_eq!(v["id"], "gn1");
    }

    #[test]
    fn protocol_got_node_complete_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotNodeComplete {
            node_id: "gn1".to_string(),
            result_summary: "done".to_string(),
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "got_node_complete");
        assert_eq!(v["result_summary"], "done");
    }

    #[test]
    fn protocol_got_node_failed_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotNodeFailed {
            node_id: "gn2".to_string(),
            error: "timeout".to_string(),
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "got_node_failed");
        assert_eq!(v["error"], "timeout");
    }

    #[test]
    fn protocol_got_expand_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotExpand {
            node_id: "gn1".to_string(),
            nodes_added: 2,
            edges_added: 1,
        };
        let pe = stream_event_to_protocol_event(&ev).unwrap();
        let v = pe.to_value().unwrap();
        assert_eq!(v["type"], "got_expand");
        assert_eq!(v["nodes_added"], 2);
        assert_eq!(v["edges_added"], 1);
    }

    // --- stream_event_to_format_a tests ---

    #[test]
    fn format_a_task_start() {
        let ev: StreamEvent<DummyState> = StreamEvent::TaskStart {
            node_id: "think".to_string(),
            namespace: None,
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TaskStart"]["node_id"], "think");
    }

    #[test]
    fn format_a_task_end_ok() {
        let ev: StreamEvent<DummyState> = StreamEvent::TaskEnd {
            node_id: "act".to_string(),
            result: Ok(()),
            namespace: None,
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TaskEnd"]["node_id"], "act");
        assert_eq!(v["TaskEnd"]["result"], "Ok");
    }

    #[test]
    fn format_a_task_end_err() {
        let ev: StreamEvent<DummyState> = StreamEvent::TaskEnd {
            node_id: "fail".to_string(),
            result: Err("boom".to_string()),
            namespace: None,
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TaskEnd"]["result"]["Err"], "boom");
    }

    #[test]
    fn format_a_usage() {
        let ev: StreamEvent<DummyState> = StreamEvent::TurnFinish {
            reason: "stop".to_string(),
            usage: crate::Usage {
                input: 10,
                output: 5,
                reasoning: None,
                cache_read: None,
                cache_write: None,
            },
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TurnFinish"]["usage"]["input"], 10);
        assert_eq!(v["TurnFinish"]["usage"]["output"], 5);
    }

    #[test]
    fn format_a_messages() {
        let ev: StreamEvent<DummyState> = StreamEvent::TextDelta {
            content: "hello".to_string(),
            metadata: StreamMetadata {
                anureo_node: "think".to_string(),
                namespace: None,
            },
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TextDelta"]["content"], "hello");
        assert_eq!(v["TextDelta"]["metadata"]["anureo_node"], "think");
    }

    #[test]
    fn format_a_values() {
        let ev: StreamEvent<DummyState> = StreamEvent::Values(DummyState(42));
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["Values"], 42);
    }

    #[test]
    fn format_a_updates() {
        let ev: StreamEvent<DummyState> = StreamEvent::Updates {
            node_id: "think".to_string(),
            state: DummyState(7),
            namespace: None,
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["Updates"]["node_id"], "think");
        assert_eq!(v["Updates"]["state"], 7);
    }

    #[test]
    fn format_a_custom() {
        let ev: StreamEvent<DummyState> = StreamEvent::Custom(serde_json::json!({"key": "value"}));
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["Custom"]["key"], "value");
    }

    #[test]
    fn format_a_checkpoint() {
        let ev: StreamEvent<DummyState> = StreamEvent::Checkpoint(CheckpointEvent {
            checkpoint_id: "cp1".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            step: 5,
            state: DummyState(99),
            thread_id: Some("t1".to_string()),
            checkpoint_ns: None,
        });
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["Checkpoint"]["checkpoint_id"], "cp1");
        assert_eq!(v["Checkpoint"]["step"], 5);
        assert_eq!(v["Checkpoint"]["state"], 99);
    }

    #[test]
    fn format_a_tot_expand() {
        let ev: StreamEvent<DummyState> = StreamEvent::TotExpand {
            candidates: vec!["a".to_string(), "b".to_string()],
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TotExpand"]["candidates"][0], "a");
    }

    #[test]
    fn format_a_tot_evaluate() {
        let ev: StreamEvent<DummyState> = StreamEvent::TotEvaluate {
            chosen: 1,
            scores: vec![0.5, 0.9],
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TotEvaluate"]["chosen"], 1);
    }

    #[test]
    fn format_a_tot_backtrack() {
        let ev: StreamEvent<DummyState> = StreamEvent::TotBacktrack {
            reason: "dead end".to_string(),
            to_depth: 2,
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TotBacktrack"]["reason"], "dead end");
        assert_eq!(v["TotBacktrack"]["to_depth"], 2);
    }

    #[test]
    fn format_a_got_plan() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotPlan {
            node_count: 3,
            edge_count: 2,
            node_ids: vec!["n1".to_string(), "n2".to_string(), "n3".to_string()],
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["GotPlan"]["node_count"], 3);
        assert_eq!(v["GotPlan"]["edge_count"], 2);
    }

    #[test]
    fn format_a_got_node_start() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotNodeStart {
            node_id: "n1".to_string(),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["GotNodeStart"]["node_id"], "n1");
    }

    #[test]
    fn format_a_got_node_complete() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotNodeComplete {
            node_id: "n1".to_string(),
            result_summary: "done".to_string(),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["GotNodeComplete"]["node_id"], "n1");
        assert_eq!(v["GotNodeComplete"]["result_summary"], "done");
    }

    #[test]
    fn format_a_got_node_failed() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotNodeFailed {
            node_id: "n2".to_string(),
            error: "timeout".to_string(),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["GotNodeFailed"]["error"], "timeout");
    }

    #[test]
    fn format_a_got_expand() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotExpand {
            node_id: "n1".to_string(),
            nodes_added: 2,
            edges_added: 3,
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["GotExpand"]["nodes_added"], 2);
    }

    #[test]
    fn format_a_tool_call() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolCall {
            call_id: Some("c1".to_string()),
            name: "read".to_string(),
            arguments: serde_json::json!({"file": "a.txt"}),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["ToolCall"]["name"], "read");
    }

    #[test]
    fn format_a_tool_start() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolStart {
            call_id: Some("c1".to_string()),
            name: "bash".to_string(),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["ToolStart"]["name"], "bash");
    }

    #[test]
    fn format_a_tool_output() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolOutput {
            call_id: Some("c1".to_string()),
            name: "bash".to_string(),
            content: "ok".to_string(),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["ToolOutput"]["content"], "ok");
    }

    #[test]
    fn format_a_tool_end() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolEnd {
            call_id: Some("c1".to_string()),
            name: "bash".to_string(),
            result: "success".to_string(),
            is_error: false,
            raw_result: None,
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["ToolEnd"]["is_error"], false);
    }
}
