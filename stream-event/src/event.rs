//! Protocol-level event types (protocol_spec §4: type + payload).
//! State-carrying variants use `serde_json::Value`; the bridge in loom serializes `S` into that.

use serde::Serialize;
use serde_json::Value;

/// Protocol event: wire shape for one stream event (type + payload).
/// Matches protocol_spec §4.2; envelope (session_id, node_id, event_id) is applied separately.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolEvent {
    NodeEnter { id: String },
    NodeExit {
        id: String,
        result: Value,
    },
    MessageChunk { content: String, id: String },
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
    },
    Values { state: Value },
    Updates { id: String, state: Value },
    Custom { value: Value },
    Checkpoint {
        checkpoint_id: String,
        timestamp: String,
        step: i64,
        state: Value,
        thread_id: Option<String>,
        checkpoint_ns: Option<String>,
    },
    TotExpand { candidates: Vec<String> },
    TotEvaluate { chosen: usize, scores: Vec<f32> },
    TotBacktrack { reason: String, to_depth: u32 },
    GotPlan {
        node_count: usize,
        edge_count: usize,
        node_ids: Vec<String>,
    },
    GotNodeStart { id: String },
    GotNodeComplete {
        id: String,
        result_summary: String,
    },
    GotNodeFailed { id: String, error: String },
    GotExpand {
        node_id: String,
        nodes_added: usize,
        edges_added: usize,
    },
}

impl ProtocolEvent {
    /// Serializes this event to a JSON object (type + payload only; no envelope).
    pub fn to_value(&self) -> Result<Value, serde_json::Error> {
        serde_json::to_value(self)
    }
}
