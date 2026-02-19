//! Streaming output protocol (protocol_spec).
//!
//! Event serialization as **type + payload** per [protocol_spec](https://github.com/loom/loom/blob/main/docs/protocol_spec.md) §4,
//! and optional **envelope** (session_id, node_id, event_id) per §2 / §7.1.

use crate::stream::{MessageChunk, StreamEvent, StreamMetadata};
use serde::Serialize;
use serde_json::{json, Value};
use std::fmt::Debug;

/// Envelope fields recommended for each message (protocol_spec §2, §7.1).
#[derive(Clone, Debug, Default)]
pub struct Envelope {
    /// Session ID; constant within a session.
    pub session_id: Option<String>,
    /// Node run ID for the current span (from node_enter to node_exit).
    pub node_id: Option<String>,
    /// Per-message sequence number; monotonically increasing within a stream.
    pub event_id: Option<u64>,
}

impl Envelope {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    pub fn with_node_id(mut self, id: impl Into<String>) -> Self {
        self.node_id = Some(id.into());
        self
    }

    pub fn with_event_id(mut self, id: u64) -> Self {
        self.event_id = Some(id);
        self
    }

    /// Merges envelope fields into the given JSON object (top-level only).
    /// Does not overwrite existing keys.
    pub fn inject_into(&self, obj: &mut Value) {
        let Some(obj) = obj.as_object_mut() else {
            return;
        };
        if let Some(ref id) = self.session_id {
            obj.entry("session_id")
                .or_insert_with(|| Value::String(id.clone()));
        }
        if let Some(ref id) = self.node_id {
            obj.entry("node_id")
                .or_insert_with(|| Value::String(id.clone()));
        }
        if let Some(id) = self.event_id {
            obj.entry("event_id")
                .or_insert_with(|| Value::Number(serde_json::Number::from(id)));
        }
    }
}

/// Converts a `StreamEvent<S>` to protocol format: top-level **type** + payload (protocol_spec §4.2).
///
/// Output shape: `{"type":"node_enter","id":"think"}`, `{"type":"message_chunk","content":"...","id":"think"}`, etc.
/// Payload uses **id** for node name (body); envelope **node_id** is separate and applied by callers via [`Envelope::inject_into`].
pub fn stream_event_to_protocol_format<S>(
    ev: &StreamEvent<S>,
) -> Result<Value, serde_json::Error>
where
    S: Serialize + Clone + Send + Sync + Debug + 'static,
{
    let obj = match ev {
        StreamEvent::TaskStart { node_id } => json!({
            "type": "node_enter",
            "id": node_id,
        }),
        StreamEvent::TaskEnd { node_id, result } => {
            let result_json = match result {
                Ok(()) => json!("Ok"),
                Err(e) => json!({ "Err": e }),
            };
            json!({
                "type": "node_exit",
                "id": node_id,
                "result": result_json,
            })
        }
        StreamEvent::Messages {
            chunk: MessageChunk { content },
            metadata: StreamMetadata { loom_node },
        } => json!({
            "type": "message_chunk",
            "content": content,
            "id": loom_node,
        }),
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        } => json!({
            "type": "usage",
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": total_tokens,
        }),
        StreamEvent::Values(state) => {
            let state_json = serde_json::to_value(state)?;
            json!({
                "type": "values",
                "state": state_json,
            })
        }
        StreamEvent::Updates { node_id, state } => {
            let state_json = serde_json::to_value(state)?;
            json!({
                "type": "updates",
                "id": node_id,
                "state": state_json,
            })
        }
        StreamEvent::Custom(v) => json!({
            "type": "custom",
            "value": v,
        }),
        StreamEvent::Checkpoint(cp) => {
            let state_json = serde_json::to_value(&cp.state)?;
            json!({
                "type": "checkpoint",
                "checkpoint_id": cp.checkpoint_id,
                "timestamp": cp.timestamp,
                "step": cp.step,
                "state": state_json,
                "thread_id": cp.thread_id,
                "checkpoint_ns": cp.checkpoint_ns,
            })
        }
        StreamEvent::TotExpand { candidates } => json!({
            "type": "tot_expand",
            "candidates": candidates,
        }),
        StreamEvent::TotEvaluate { chosen, scores } => json!({
            "type": "tot_evaluate",
            "chosen": chosen,
            "scores": scores,
        }),
        StreamEvent::TotBacktrack { reason, to_depth } => json!({
            "type": "tot_backtrack",
            "reason": reason,
            "to_depth": to_depth,
        }),
        StreamEvent::GotPlan {
            node_count,
            edge_count,
            node_ids,
        } => json!({
            "type": "got_plan",
            "node_count": node_count,
            "edge_count": edge_count,
            "node_ids": node_ids,
        }),
        StreamEvent::GotNodeStart { node_id } => json!({
            "type": "got_node_start",
            "id": node_id,
        }),
        StreamEvent::GotNodeComplete {
            node_id,
            result_summary,
        } => json!({
            "type": "got_node_complete",
            "id": node_id,
            "result_summary": result_summary,
        }),
        StreamEvent::GotNodeFailed { node_id, error } => json!({
            "type": "got_node_failed",
            "id": node_id,
            "error": error,
        }),
        StreamEvent::GotExpand {
            node_id,
            nodes_added,
            edges_added,
        } => json!({
            "type": "got_expand",
            "node_id": node_id,
            "nodes_added": nodes_added,
            "edges_added": edges_added,
        }),
    };
    Ok(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::StreamMetadata;

    #[derive(Clone, Debug, serde::Serialize)]
    struct DummyState(i32);

    #[test]
    fn node_enter_format() {
        let ev: StreamEvent<DummyState> =
            StreamEvent::TaskStart { node_id: "think".to_string() };
        let v = stream_event_to_protocol_format(&ev).unwrap();
        assert_eq!(v["type"], "node_enter");
        assert_eq!(v["id"], "think");
    }

    #[test]
    fn node_exit_ok_format() {
        let ev: StreamEvent<DummyState> =
            StreamEvent::TaskEnd { node_id: "act".to_string(), result: Ok(()) };
        let v = stream_event_to_protocol_format(&ev).unwrap();
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
        let v = stream_event_to_protocol_format(&ev).unwrap();
        assert_eq!(v["type"], "message_chunk");
        assert_eq!(v["content"], "hello");
        assert_eq!(v["id"], "think");
    }

    #[test]
    fn envelope_inject() {
        let mut obj = json!({"type":"node_enter","id":"think"});
        let env = Envelope::new()
            .with_session_id("sess-1")
            .with_node_id("run-think-1")
            .with_event_id(1);
        env.inject_into(&mut obj);
        assert_eq!(obj["session_id"], "sess-1");
        assert_eq!(obj["node_id"], "run-think-1");
        assert_eq!(obj["event_id"], 1);
        assert_eq!(obj["type"], "node_enter");
    }

    #[test]
    fn usage_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        };
        let v = stream_event_to_protocol_format(&ev).unwrap();
        assert_eq!(v["type"], "usage");
        assert_eq!(v["prompt_tokens"], 10);
        assert_eq!(v["total_tokens"], 15);
    }

    #[test]
    fn values_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Values(DummyState(42));
        let v = stream_event_to_protocol_format(&ev).unwrap();
        assert_eq!(v["type"], "values");
        assert_eq!(v["state"], 42);
    }

    #[test]
    fn updates_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Updates {
            node_id: "act".to_string(),
            state: DummyState(1),
        };
        let v = stream_event_to_protocol_format(&ev).unwrap();
        assert_eq!(v["type"], "updates");
        assert_eq!(v["id"], "act");
        assert_eq!(v["state"], 1);
    }

    #[test]
    fn custom_format() {
        let ev: StreamEvent<DummyState> =
            StreamEvent::Custom(serde_json::json!({"foo": "bar"}));
        let v = stream_event_to_protocol_format(&ev).unwrap();
        assert_eq!(v["type"], "custom");
        assert_eq!(v["value"]["foo"], "bar");
    }

    #[test]
    fn node_exit_err_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TaskEnd {
            node_id: "fail".to_string(),
            result: Err("boom".to_string()),
        };
        let v = stream_event_to_protocol_format(&ev).unwrap();
        assert_eq!(v["type"], "node_exit");
        assert_eq!(v["id"], "fail");
        assert_eq!(v["result"]["Err"], "boom");
    }
}
