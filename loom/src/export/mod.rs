//! Export utilities: StreamEvent to format A JSON (EXPORT_SPEC §2).
//!
//! Converts [`StreamEvent`] to a single JSON object per event for WebSocket streaming.

use crate::stream::{MessageChunk, StreamEvent, StreamMetadata};
use serde::Serialize;
use serde_json::{json, Value};
use std::fmt::Debug;

/// Converts a `StreamEvent<S>` to format A JSON (single-key object, externally tagged).
///
/// Output shape: `{"TaskStart":{"node_id":"think"}}`, `{"Usage":{...}}`, etc.
/// Aligned with [EXPORT_SPEC](https://github.com/loom/loom/blob/main/docs/EXPORT_SPEC.md) §2.
pub fn stream_event_to_format_a<S>(ev: &StreamEvent<S>) -> Result<Value, serde_json::Error>
where
    S: Serialize + Clone + Send + Sync + Debug + 'static,
{
    let obj = match ev {
        StreamEvent::Values(state) => {
            let state_json = serde_json::to_value(state)?;
            json!({ "Values": state_json })
        }
        StreamEvent::Updates { node_id, state } => {
            let state_json = serde_json::to_value(state)?;
            json!({ "Updates": { "node_id": node_id, "state": state_json } })
        }
        StreamEvent::Messages {
            chunk:
                MessageChunk { content },
            metadata:
                StreamMetadata { loom_node },
        } => json!({
            "Messages": {
                "chunk": { "content": content },
                "metadata": { "loom_node": loom_node }
            }
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
        StreamEvent::TaskStart { node_id } => json!({ "TaskStart": { "node_id": node_id } }),
        StreamEvent::TaskEnd { node_id, result } => {
            let result_json = match result {
                Ok(()) => json!("Ok"),
                Err(e) => json!({ "Err": e }),
            };
            json!({ "TaskEnd": { "node_id": node_id, "result": result_json } })
        }
        StreamEvent::TotExpand { candidates } => json!({ "TotExpand": { "candidates": candidates } }),
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
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        } => json!({
            "Usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": total_tokens
            }
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
    fn task_start_format() {
        let ev: StreamEvent<DummyState> =
            StreamEvent::TaskStart { node_id: "think".to_string() };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TaskStart"]["node_id"], "think");
    }

    #[test]
    fn task_end_ok_format() {
        let ev: StreamEvent<DummyState> =
            StreamEvent::TaskEnd { node_id: "act".to_string(), result: Ok(()) };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TaskEnd"]["node_id"], "act");
        assert_eq!(v["TaskEnd"]["result"], "Ok");
    }

    #[test]
    fn task_end_err_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TaskEnd {
            node_id: "fail".to_string(),
            result: Err("boom".to_string()),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TaskEnd"]["result"]["Err"], "boom");
    }

    #[test]
    fn usage_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["Usage"]["prompt_tokens"], 10);
        assert_eq!(v["Usage"]["completion_tokens"], 5);
    }

    #[test]
    fn messages_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Messages {
            chunk: crate::stream::MessageChunk {
                content: "hello".to_string(),
            },
            metadata: StreamMetadata {
                loom_node: "think".to_string(),
            },
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["Messages"]["chunk"]["content"], "hello");
        assert_eq!(v["Messages"]["metadata"]["loom_node"], "think");
    }
}
