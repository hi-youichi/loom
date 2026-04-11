//! Export utilities: StreamEvent to format A JSON (EXPORT_SPEC §2).
//!
//! Converts [`StreamEvent`] to a single JSON object per event for WebSocket streaming.

use crate::stream::{StreamEvent, StreamMetadata};
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
        StreamEvent::Updates { node_id, state, namespace } => {
            let state_json = serde_json::to_value(state)?;
            json!({ "Updates": { "node_id": node_id, "state": state_json, "namespace": namespace } })
        }
        StreamEvent::Messages {
            chunk,
            metadata: StreamMetadata { loom_node, namespace },
        } => json!({
            "Messages": {
                "chunk": { "content": chunk.content, "kind": format!("{:?}", chunk.kind) },
                "metadata": { "loom_node": loom_node, "namespace": namespace }
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
        StreamEvent::TaskStart { node_id, namespace } => json!({ "TaskStart": { "node_id": node_id, "namespace": namespace } }),
        StreamEvent::TaskEnd { node_id, result, namespace } => {
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
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
            ..
        } => json!({
            "Usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": total_tokens
            }
        }),
        StreamEvent::ToolCallChunk {
            call_id,
            name,
            arguments_delta,
        } => json!({
            "ToolCallChunk": { "call_id": call_id, "name": name, "arguments_delta": arguments_delta }
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
        StreamEvent::ToolApproval {
            call_id,
            name,
            arguments,
        } => json!({
            "ToolApproval": { "call_id": call_id, "name": name, "arguments": arguments }
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
        let ev: StreamEvent<DummyState> = StreamEvent::TaskStart {
            node_id: "think".to_string(),
            namespace: None,
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TaskStart"]["node_id"], "think");
    }

    #[test]
    fn task_end_ok_format() {
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
    fn task_end_err_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TaskEnd {
            node_id: "fail".to_string(),
            result: Err("boom".to_string()),
            namespace: None,
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
            prefill_duration: None,
            decode_duration: None,
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["Usage"]["prompt_tokens"], 10);
        assert_eq!(v["Usage"]["completion_tokens"], 5);
    }

    #[test]
    fn messages_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Messages {
            chunk: crate::stream::MessageChunk::message("hello"),
            metadata: StreamMetadata {
                loom_node: "think".to_string(),
                namespace: None,
            },
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["Messages"]["chunk"]["content"], "hello");
        assert_eq!(v["Messages"]["metadata"]["loom_node"], "think");
    }

    #[test]
    fn values_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Values(DummyState(42));
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["Values"], 42);
    }

    #[test]
    fn updates_format() {
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
    fn custom_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Custom(serde_json::json!({"key": "value"}));
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["Custom"]["key"], "value");
    }

    #[test]
    fn checkpoint_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::Checkpoint(crate::stream::CheckpointEvent {
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
    fn tot_expand_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TotExpand {
            candidates: vec!["a".to_string(), "b".to_string()],
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TotExpand"]["candidates"][0], "a");
    }

    #[test]
    fn tot_evaluate_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TotEvaluate {
            chosen: 1,
            scores: vec![0.5, 0.9],
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TotEvaluate"]["chosen"], 1);
    }

    #[test]
    fn tot_backtrack_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::TotBacktrack {
            reason: "dead end".to_string(),
            to_depth: 2,
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["TotBacktrack"]["reason"], "dead end");
        assert_eq!(v["TotBacktrack"]["to_depth"], 2);
    }

    #[test]
    fn got_plan_format() {
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
    fn got_node_start_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotNodeStart {
            node_id: "n1".to_string(),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["GotNodeStart"]["node_id"], "n1");
    }

    #[test]
    fn got_node_complete_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotNodeComplete {
            node_id: "n1".to_string(),
            result_summary: "done".to_string(),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["GotNodeComplete"]["node_id"], "n1");
        assert_eq!(v["GotNodeComplete"]["result_summary"], "done");
    }

    #[test]
    fn got_node_failed_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotNodeFailed {
            node_id: "n2".to_string(),
            error: "timeout".to_string(),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["GotNodeFailed"]["error"], "timeout");
    }

    #[test]
    fn got_expand_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::GotExpand {
            node_id: "n1".to_string(),
            nodes_added: 2,
            edges_added: 3,
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["GotExpand"]["nodes_added"], 2);
    }

    #[test]
    fn tool_call_chunk_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolCallChunk {
            call_id: Some("c1".to_string()),
            name: Some("bash".to_string()),
            arguments_delta: "{\"cmd\"".to_string(),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["ToolCallChunk"]["name"], "bash");
    }

    #[test]
    fn tool_call_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolCall {
            call_id: Some("c1".to_string()),
            name: "read".to_string(),
            arguments: serde_json::json!({"file": "a.txt"}),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["ToolCall"]["name"], "read");
    }

    #[test]
    fn tool_start_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolStart {
            call_id: Some("c1".to_string()),
            name: "bash".to_string(),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["ToolStart"]["name"], "bash");
    }

    #[test]
    fn tool_output_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolOutput {
            call_id: Some("c1".to_string()),
            name: "bash".to_string(),
            content: "ok".to_string(),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["ToolOutput"]["content"], "ok");
    }

    #[test]
    fn tool_end_format() {
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

    #[test]
    fn tool_approval_format() {
        let ev: StreamEvent<DummyState> = StreamEvent::ToolApproval {
            call_id: Some("c1".to_string()),
            name: "bash".to_string(),
            arguments: serde_json::json!({}),
        };
        let v = stream_event_to_format_a(&ev).unwrap();
        assert_eq!(v["ToolApproval"]["name"], "bash");
    }
}
