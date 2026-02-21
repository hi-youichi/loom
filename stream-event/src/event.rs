//! Protocol-level event types (protocol_spec §4: type + payload).
//! State-carrying variants use `serde_json::Value`; the bridge in loom serializes `S` into that.

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
    NodeEnter {
        id: String,
    },
    NodeExit {
        id: String,
        result: Value,
    },
    MessageChunk {
        content: String,
        id: String,
    },
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
    },
    Values {
        state: Value,
    },
    Updates {
        id: String,
        state: Value,
    },
    Custom {
        value: Value,
    },
    Checkpoint {
        checkpoint_id: String,
        timestamp: String,
        step: i64,
        state: Value,
        thread_id: Option<String>,
        checkpoint_ns: Option<String>,
    },
    TotExpand {
        candidates: Vec<String>,
    },
    TotEvaluate {
        chosen: usize,
        scores: Vec<f32>,
    },
    TotBacktrack {
        reason: String,
        to_depth: u32,
    },
    GotPlan {
        node_count: usize,
        edge_count: usize,
        node_ids: Vec<String>,
    },
    GotNodeStart {
        id: String,
    },
    GotNodeComplete {
        id: String,
        result_summary: String,
    },
    GotNodeFailed {
        id: String,
        error: String,
    },
    GotExpand {
        node_id: String,
        nodes_added: usize,
        edges_added: usize,
    },
    ToolCallChunk {
        call_id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    ToolCall {
        call_id: Option<String>,
        name: String,
        arguments: Value,
    },
    ToolStart {
        call_id: Option<String>,
        name: String,
    },
    ToolOutput {
        call_id: Option<String>,
        name: String,
        content: String,
    },
    ToolEnd {
        call_id: Option<String>,
        name: String,
        result: String,
        is_error: bool,
    },
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
