//! Envelope (session_id, node_id, event_id) per protocol_spec §2 / §7.1.
//! EnvelopeState tracks current node and injects envelope into each event.

use crate::event::ProtocolEvent;
use serde_json::Value;

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
    /// On `type == "node_enter"`, updates current_node_id from the event's `id`.
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

/// Converts a protocol event to JSON and injects envelope using the given state.
/// Returns the final value (type + payload + session_id, node_id, event_id).
pub fn to_json(
    event: &ProtocolEvent,
    state: &mut EnvelopeState,
) -> Result<Value, serde_json::Error> {
    let mut value = event.to_value()?;
    state.inject_into(&mut value);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ProtocolEvent;

    #[test]
    fn envelope_inject() {
        let mut obj = serde_json::json!({"type":"node_enter","id":"think"});
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
    fn to_json_injects_envelope() {
        let ev = ProtocolEvent::NodeEnter {
            id: "think".to_string(),
        };
        let mut state = EnvelopeState::new("run-123".to_string());
        let value = to_json(&ev, &mut state).unwrap();
        assert_eq!(value["type"], "node_enter");
        assert_eq!(value["id"], "think");
        assert_eq!(value["session_id"], "run-123");
        assert_eq!(value["event_id"], 1);
    }
}
