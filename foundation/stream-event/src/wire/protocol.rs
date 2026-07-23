//! Protocol-level event types ([protocol_spec §4](https://github.com/loom/loom/blob/main/docs/protocol_spec.md#4-message-structure-events): type + payload).
//! State-carrying variants use `serde_json::Value`; the bridge in loom serializes `S` into that.
//!
//! # Architecture
//!
//! Single pipeline (loom produces, this crate defines wire shape and envelope injection):
//!
//! ```text
//!   [loom]                                    [stream-event]
//!   Agent emits StreamEvent<S>                This crate
//!
//!   StreamEvent<S>  ──stream_event_to_protocol_event()──►  ProtocolEvent  ──to_json(ev, &mut state)──►  JSON frame
//!   (internal)         (in loom)                           (this enum)    (envelope injected here)    (on wire)
//! ```
//!
//! - **ProtocolEvent**: wire shape (type + payload only). Loom converts `StreamEvent<S>` into it; [`to_json`] takes it and produces the final frame.
//! - **to_json(event, &mut EnvelopeState)**: serializes the event and injects `session_id`, `node_id`, `event_id` from state (see [`crate::envelope`] and [`EnvelopeState`]).
//!
//! [`to_json`]: crate::to_json
//! [`EnvelopeState`]: crate::EnvelopeState

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Protocol event: wire shape for one stream event (type + payload).
/// Matches [protocol_spec §4.2](https://github.com/loom/loom/blob/main/docs/protocol_spec.md#42-event-types-and-payloads); envelope (`session_id`, `node_id`, `event_id`) is applied separately via [`to_json`](crate::to_json) and [`EnvelopeState`](crate::EnvelopeState).
///
/// Note on naming:
/// - `id` in payload = node name (e.g. "think", "act"); see [`EnvelopeState`](crate::EnvelopeState) for how envelope `node_id` is derived from `NodeEnter.id`.
/// - `got_expand` keeps payload field name `node_id` by protocol definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolEvent {
    /// Node run started. Emitted when the agent begins executing a node (e.g. think, act).
    /// `id` is the node name; the envelope `node_id` is set from this until the next [`NodeExit`](Self::NodeExit).
    NodeEnter {
        /// Node name (e.g. `"think"`, `"act"`).
        id: String,
    },
    /// Node run ended. Emitted when a node finishes. Pairs with [`NodeEnter`](Self::NodeEnter).
    /// `result` is `"Ok"` or `{"Err": "<message>"}`.
    NodeExit {
        /// Node name that just exited.
        id: String,
        /// `"Ok"` on success, or `{"Err": string}` on failure.
        result: Value,
    },
    TextDelta {
        content: String,
        id: String,
    },
    ReasoningDelta {
        reasoning_id: String,
        content: String,
        id: String,
    },
    /// Token usage for the last LLM call in this node.
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
    },
    /// Full state snapshot. Emitted to replace client state with the given graph state.
    /// Shape depends on agent type (ReAct, ToT, GoT, etc.). Contrast with [`Updates`](Self::Updates) (incremental).
    Values {
        /// Serialized graph state.
        state: Value,
    },
    /// State update after a node merge. Emitted when one node’s result is merged into state.
    /// See [`Values`](Self::Values) for a full state snapshot.
    Updates {
        /// Node name that produced this update.
        id: String,
        /// New or partial state after merge.
        state: Value,
    },
    /// Custom JSON payload. For extension points or debugging.
    Custom {
        /// Arbitrary JSON value.
        value: Value,
    },
    /// Checkpoint for persistence. Contains a serialized state snapshot and metadata.
    /// Reply envelope can be built with [`EnvelopeState::reply_envelope`](crate::EnvelopeState::reply_envelope).
    Checkpoint {
        checkpoint_id: String,
        timestamp: String,
        step: i64,
        state: Value,
        thread_id: Option<String>,
        checkpoint_ns: Option<String>,
    },
    /// **Tree of Thought**: expansion step. The model produced multiple candidate next steps.
    TotExpand {
        /// Candidate strings (e.g. thought continuations).
        candidates: Vec<String>,
    },
    /// **Tree of Thought**: evaluation step. One candidate was chosen; optional scores for all.
    /// References the candidate list from the previous [`TotExpand`](Self::TotExpand).
    TotEvaluate {
        /// Index of the chosen candidate in the previous expand list.
        chosen: usize,
        /// Scores for each candidate (may be empty).
        scores: Vec<f32>,
    },
    /// **Tree of Thought**: backtrack. Search is rewinding to an earlier depth.
    TotBacktrack {
        reason: String,
        /// Depth to rewind to.
        to_depth: u32,
    },
    /// **Graph of Thought**: plan created. The execution graph has been laid out.
    /// Followed by [`GotNodeStart`](Self::GotNodeStart) / [`GotNodeComplete`](Self::GotNodeComplete) / [`GotNodeFailed`](Self::GotNodeFailed) for each node.
    GotPlan {
        node_count: usize,
        edge_count: usize,
        /// IDs of all nodes in the plan.
        node_ids: Vec<String>,
    },
    /// **Graph of Thought**: a plan node has started executing. See [`GotPlan`](Self::GotPlan).
    GotNodeStart {
        /// Node ID that started.
        id: String,
    },
    /// **Graph of Thought**: a plan node completed successfully.
    GotNodeComplete {
        id: String,
        result_summary: String,
    },
    /// **Graph of Thought**: a plan node failed.
    GotNodeFailed {
        id: String,
        error: String,
    },
    /// **Graph of Thought**: adaptive expand. New nodes/edges were added from a node.
    /// Payload uses `node_id` (not `id`) per protocol.
    GotExpand {
        /// Node that triggered the expand.
        node_id: String,
        nodes_added: usize,
        edges_added: usize,
    },
    /// Complete tool call: name and full arguments. Emitted when the model finishes the call.
    /// Followed by [`ToolStart`](Self::ToolStart) → [`ToolOutput`](Self::ToolOutput) (zero or more) → [`ToolEnd`](Self::ToolEnd).
    ToolCall {
        call_id: Option<String>,
        name: String,
        arguments: Value,
    },
    /// Tool execution has started. The runner is about to invoke the tool. See [`ToolCall`](Self::ToolCall).
    ToolStart {
        call_id: Option<String>,
        name: String,
    },
    /// Content produced by the tool (e.g. stdout). May be sent multiple times per call. See [`ToolEnd`](Self::ToolEnd).
    ToolOutput {
        call_id: Option<String>,
        name: String,
        content: String,
    },
    /// Tool execution finished. Contains the final result or error text. Ends the sequence started by [`ToolStart`](Self::ToolStart).
    ToolEnd {
        call_id: Option<String>,
        name: String,
        /// Result text (success or error message), possibly normalized/truncated for display.
        result: String,
        /// Whether the tool reported an error.
        is_error: bool,
        /// Full un-normalized result text. When set, ACP agents use this for raw_output
        /// instead of `result` (which may be a head-tail excerpt or file reference).
        #[serde(skip_serializing_if = "Option::is_none")]
        raw_result: Option<String>,
    },
}

impl ProtocolEvent {
    /// Serializes this event to a JSON object (type + payload only; no envelope).
    ///
    /// Use crate-level [`crate::to_json`] when you need envelope fields injected.
    pub fn to_value(&self) -> Result<Value, serde_json::Error> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::ProtocolEvent;
    use serde_json::json;

    #[test]
    fn text_delta_serializes_with_type_content_id() {
        let event = ProtocolEvent::TextDelta {
            content: "final reply".to_string(),
            id: "think".to_string(),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "text_delta");
        assert_eq!(v["content"], "final reply");
        assert_eq!(v["id"], "think");
    }

    #[test]
    fn reasoning_delta_serializes_with_type_content_id() {
        let event = ProtocolEvent::ReasoningDelta {
            reasoning_id: "r0".to_string(),
            content: "reasoning step".to_string(),
            id: "think".to_string(),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "reasoning_delta");
        assert_eq!(v["reasoning_id"], "r0");
        assert_eq!(v["content"], "reasoning step");
        assert_eq!(v["id"], "think");
    }

    #[test]
    fn updates_uses_payload_id_field() {
        let event = ProtocolEvent::Updates {
            id: "act".to_string(),
            state: json!({"foo":"bar"}),
        };
        let value = event.to_value().unwrap();

        assert_eq!(value["type"], "updates");
        assert_eq!(value["id"], "act");
        assert!(value.get("node_id").is_none());
    }

    #[test]
    fn got_expand_uses_payload_node_id_field() {
        let event = ProtocolEvent::GotExpand {
            node_id: "n-3".to_string(),
            nodes_added: 2,
            edges_added: 1,
        };
        let value = event.to_value().unwrap();

        assert_eq!(value["type"], "got_expand");
        assert_eq!(value["node_id"], "n-3");
        assert!(value.get("id").is_none());
    }

    #[test]
    fn tool_call_format() {
        let event = ProtocolEvent::ToolCall {
            call_id: Some("call_abc".to_string()),
            name: "list_dir".to_string(),
            arguments: json!({"path": "./src"}),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tool_call");
        assert_eq!(v["call_id"], "call_abc");
        assert_eq!(v["name"], "list_dir");
        assert_eq!(v["arguments"]["path"], "./src");
    }

    #[test]
    fn tool_call_without_call_id() {
        let event = ProtocolEvent::ToolCall {
            call_id: None,
            name: "bash".to_string(),
            arguments: json!({"cmd": "ls"}),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tool_call");
        assert!(v["call_id"].is_null());
        assert_eq!(v["name"], "bash");
    }

    #[test]
    fn tool_start_format() {
        let event = ProtocolEvent::ToolStart {
            call_id: Some("call_1".to_string()),
            name: "list_dir".to_string(),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tool_start");
        assert_eq!(v["call_id"], "call_1");
        assert_eq!(v["name"], "list_dir");
    }

    #[test]
    fn tool_output_format() {
        let event = ProtocolEvent::ToolOutput {
            call_id: Some("call_1".to_string()),
            name: "bash".to_string(),
            content: "Compiling loom v0.1.0\n".to_string(),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tool_output");
        assert_eq!(v["call_id"], "call_1");
        assert_eq!(v["name"], "bash");
        assert_eq!(v["content"], "Compiling loom v0.1.0\n");
    }

    #[test]
    fn tool_end_success_format() {
        let event = ProtocolEvent::ToolEnd {
            call_id: Some("call_1".to_string()),
            name: "list_dir".to_string(),
            result: "main.rs, lib.rs (2 entries)".to_string(),
            is_error: false,
            raw_result: None,
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tool_end");
        assert_eq!(v["call_id"], "call_1");
        assert_eq!(v["name"], "list_dir");
        assert_eq!(v["result"], "main.rs, lib.rs (2 entries)");
        assert_eq!(v["is_error"], false);
    }

    #[test]
    fn tool_end_error_format() {
        let event = ProtocolEvent::ToolEnd {
            call_id: Some("call_2".to_string()),
            name: "bash".to_string(),
            result: "Error: command not found".to_string(),
            is_error: true,
            raw_result: None,
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tool_end");
        assert_eq!(v["is_error"], true);
        assert_eq!(v["result"], "Error: command not found");
    }

    // ── Previously untested variants ──

    #[test]
    fn node_enter_serializes() {
        let event = ProtocolEvent::NodeEnter {
            id: "think".to_string(),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "node_enter");
        assert_eq!(v["id"], "think");
    }

    #[test]
    fn node_exit_ok() {
        let event = ProtocolEvent::NodeExit {
            id: "act".to_string(),
            result: json!("Ok"),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "node_exit");
        assert_eq!(v["id"], "act");
        assert_eq!(v["result"], "Ok");
    }

    #[test]
    fn node_exit_err() {
        let event = ProtocolEvent::NodeExit {
            id: "think".to_string(),
            result: json!({"Err": "timeout"}),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["result"]["Err"], "timeout");
    }

    #[test]
    fn usage_serializes() {
        let event = ProtocolEvent::Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "usage");
        assert_eq!(v["prompt_tokens"], 100);
        assert_eq!(v["completion_tokens"], 50);
        assert_eq!(v["total_tokens"], 150);
    }

    #[test]
    fn values_serializes() {
        let event = ProtocolEvent::Values {
            state: json!({"step": 3, "done": false}),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "values");
        assert_eq!(v["state"]["step"], 3);
    }

    #[test]
    fn custom_serializes() {
        let event = ProtocolEvent::Custom {
            value: json!({"debug": true}),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "custom");
        assert_eq!(v["value"]["debug"], true);
    }

    #[test]
    fn checkpoint_serializes() {
        let event = ProtocolEvent::Checkpoint {
            checkpoint_id: "cp-1".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            step: 5,
            state: json!({"x": 1}),
            thread_id: Some("t-1".to_string()),
            checkpoint_ns: None,
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "checkpoint");
        assert_eq!(v["checkpoint_id"], "cp-1");
        assert_eq!(v["step"], 5);
        assert_eq!(v["thread_id"], "t-1");
        assert!(v["checkpoint_ns"].is_null());
    }

    #[test]
    fn tot_expand_serializes() {
        let event = ProtocolEvent::TotExpand {
            candidates: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tot_expand");
        assert_eq!(v["candidates"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn tot_evaluate_serializes() {
        let event = ProtocolEvent::TotEvaluate {
            chosen: 1,
            scores: vec![0.5, 0.9, 0.3],
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tot_evaluate");
        assert_eq!(v["chosen"], 1);
    }

    #[test]
    fn tot_backtrack_serializes() {
        let event = ProtocolEvent::TotBacktrack {
            reason: "dead end".to_string(),
            to_depth: 2,
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tot_backtrack");
        assert_eq!(v["to_depth"], 2);
    }

    #[test]
    fn got_plan_serializes() {
        let event = ProtocolEvent::GotPlan {
            node_count: 3,
            edge_count: 2,
            node_ids: vec!["n1".to_string(), "n2".to_string(), "n3".to_string()],
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "got_plan");
        assert_eq!(v["node_count"], 3);
        assert_eq!(v["edge_count"], 2);
    }

    #[test]
    fn got_node_start_serializes() {
        let event = ProtocolEvent::GotNodeStart {
            id: "n-1".to_string(),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "got_node_start");
        assert_eq!(v["id"], "n-1");
    }

    #[test]
    fn got_node_complete_serializes() {
        let event = ProtocolEvent::GotNodeComplete {
            id: "n-1".to_string(),
            result_summary: "done".to_string(),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "got_node_complete");
        assert_eq!(v["result_summary"], "done");
    }

    #[test]
    fn got_node_failed_serializes() {
        let event = ProtocolEvent::GotNodeFailed {
            id: "n-2".to_string(),
            error: "crashed".to_string(),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "got_node_failed");
        assert_eq!(v["error"], "crashed");
    }

    #[test]
    fn tool_end_with_raw_result() {
        let event = ProtocolEvent::ToolEnd {
            call_id: Some("c-1".to_string()),
            name: "read".to_string(),
            result: "file contents...".to_string(),
            is_error: false,
            raw_result: Some("full file contents here".to_string()),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["raw_result"], "full file contents here");
    }

    #[test]
    fn tool_end_without_raw_result_skips_field() {
        let event = ProtocolEvent::ToolEnd {
            call_id: Some("c-1".to_string()),
            name: "read".to_string(),
            result: "ok".to_string(),
            is_error: false,
            raw_result: None,
        };
        let v = event.to_value().unwrap();
        assert!(v.get("raw_result").is_none());
    }

    #[test]
    fn protocol_event_roundtrip_serde() {
        let event = ProtocolEvent::TextDelta {
            content: "test".to_string(),
            id: "node".to_string(),
        };
        let json_str = serde_json::to_string(&event).unwrap();
        let parsed: ProtocolEvent = serde_json::from_str(&json_str).unwrap();
        let v = parsed.to_value().unwrap();
        assert_eq!(v["content"], "test");
    }
}
