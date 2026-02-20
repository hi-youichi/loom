//! Envelope state for one run: session_id, current node run id, next event_id.
//! Used when streaming events to inject envelope (protocol_spec §2 / §7.1) and to build reply envelope (§5).

use crate::protocol::stream::Envelope;
use serde_json::Value;

/// Envelope state for one run: session_id, current node run id, next event_id.
pub struct EnvelopeState {
    pub session_id: String,
    pub current_node_id: String,
    pub node_run_seq: u64,
    pub next_event_id: u64,
}

impl EnvelopeState {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            current_node_id: String::new(),
            node_run_seq: 0,
            next_event_id: 1,
        }
    }

    /// Injects envelope into the event value and advances state.
    pub fn inject_into(&mut self, value: &mut Value) {
        if let Some(t) = value.get("type").and_then(|v| v.as_str()) {
            if t == "node_enter" {
                let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("");
                self.current_node_id = format!("run-{}-{}", id, self.node_run_seq);
                self.node_run_seq += 1;
            }
        }
        let node_id = if self.current_node_id.is_empty() {
            "run-0"
        } else {
            self.current_node_id.as_str()
        };
        let env = Envelope::new()
            .with_session_id(&self.session_id)
            .with_node_id(node_id)
            .with_event_id(self.next_event_id);
        self.next_event_id += 1;
        env.inject_into(value);
    }

    /// Builds the envelope for the reply line (protocol_spec §5).
    pub fn reply_envelope(&self) -> Envelope {
        let node_id = if self.current_node_id.is_empty() {
            "run-0"
        } else {
            self.current_node_id.as_str()
        };
        Envelope::new()
            .with_session_id(&self.session_id)
            .with_node_id(node_id)
            .with_event_id(self.next_event_id)
    }
}
