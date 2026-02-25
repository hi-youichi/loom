//! Protocol-level event types (protocol_spec §4: type + payload).
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
//! - **to_json(event, &mut EnvelopeState)**: serializes the event and injects `session_id`, `node_id`, `event_id` from state (see [`crate::envelope`]).
//!
//! [`to_json`]: crate::to_json

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Protocol event: wire shape for one stream event (type + payload).
/// Matches protocol_spec §4.2; envelope (session_id, node_id, event_id) is applied separately.
///
/// Note on naming:
/// - `id` in payload means node name (e.g. "think", "act")
/// - `node_id` in envelope means node-run span id
/// - `got_expand` keeps payload field name `node_id` by protocol definition
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolEvent {
    /// Node run started. Emitted when the agent begins executing a node (e.g. think, act).
    /// `id` is the node name; the envelope `node_id` is set from this until the next `NodeExit`.
    NodeEnter {
        /// Node name (e.g. `"think"`, `"act"`).
        id: String,
    },
    /// Node run ended. Emitted when a node finishes.
    /// `result` is `"Ok"` or `{"Err": "<message>"}`.
    NodeExit {
        /// Node name that just exited.
        id: String,
        /// `"Ok"` on success, or `{"Err": string}` on failure.
        result: Value,
    },
    /// One chunk of LLM-generated text. Streamed during completion.
    /// `id` is the name of the node producing this content.
    MessageChunk {
        /// Incremental text from the model.
        content: String,
        /// Producing node name (e.g. `"think"`).
        id: String,
    },
    /// Token usage for the last LLM call in this node.
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
    },
    /// Full state snapshot. Emitted to replace client state with the given graph state.
    /// Shape depends on agent type (ReAct, ToT, GoT, etc.).
    Values {
        /// Serialized graph state.
        state: Value,
    },
    /// State update after a node merge. Emitted when one node’s result is merged into state.
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
    GotPlan {
        node_count: usize,
        edge_count: usize,
        /// IDs of all nodes in the plan.
        node_ids: Vec<String>,
    },
    /// **Graph of Thought**: a plan node has started executing.
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
    GotExpand {
        /// Node that triggered the expand.
        node_id: String,
        nodes_added: usize,
        edges_added: usize,
    },
    /// Tool call arguments streamed incrementally (e.g. streaming JSON).
    /// First chunk usually has `call_id` and `name`; later chunks may have only `arguments_delta`.
    ToolCallChunk {
        call_id: Option<String>,
        name: Option<String>,
        /// Incremental JSON or text for the arguments.
        arguments_delta: String,
    },
    /// Complete tool call: name and full arguments. Emitted when the model finishes the call.
    ToolCall {
        call_id: Option<String>,
        name: String,
        arguments: Value,
    },
    /// Tool execution has started. The runner is about to invoke the tool.
    ToolStart {
        call_id: Option<String>,
        name: String,
    },
    /// Content produced by the tool (e.g. stdout). May be sent multiple times per call.
    ToolOutput {
        call_id: Option<String>,
        name: String,
        content: String,
    },
    /// Tool execution finished. Contains the final result or error text.
    ToolEnd {
        call_id: Option<String>,
        name: String,
        /// Result text (success or error message).
        result: String,
        /// Whether the tool reported an error.
        is_error: bool,
    },
    /// Tool call awaiting user approval (e.g. destructive or privileged actions).
    /// Contains the tool name and arguments for the client to confirm or reject.
    ToolApproval {
        call_id: Option<String>,
        name: String,
        arguments: Value,
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
    fn tool_call_chunk_format() {
        let event = ProtocolEvent::ToolCallChunk {
            call_id: Some("call_abc".to_string()),
            name: Some("bash".to_string()),
            arguments_delta: "{\"cmd\":\"cargo".to_string(),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tool_call_chunk");
        assert_eq!(v["call_id"], "call_abc");
        assert_eq!(v["name"], "bash");
        assert_eq!(v["arguments_delta"], "{\"cmd\":\"cargo");
    }

    #[test]
    fn tool_call_chunk_null_name_on_subsequent_delta() {
        let event = ProtocolEvent::ToolCallChunk {
            call_id: Some("call_abc".to_string()),
            name: None,
            arguments_delta: " build\"}".to_string(),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tool_call_chunk");
        assert!(v["name"].is_null());
        assert_eq!(v["arguments_delta"], " build\"}");
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
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tool_end");
        assert_eq!(v["is_error"], true);
        assert_eq!(v["result"], "Error: command not found");
    }

    #[test]
    fn tool_approval_format() {
        let event = ProtocolEvent::ToolApproval {
            call_id: Some("call_2".to_string()),
            name: "delete_file".to_string(),
            arguments: json!({"path": "./important.txt"}),
        };
        let v = event.to_value().unwrap();
        assert_eq!(v["type"], "tool_approval");
        assert_eq!(v["call_id"], "call_2");
        assert_eq!(v["name"], "delete_file");
        assert_eq!(v["arguments"]["path"], "./important.txt");
    }
}
