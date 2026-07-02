//! Envelope (session_id, node_id, event_id) per protocol_spec §2 / §7.1.
//! EnvelopeState tracks current node and injects envelope into each event.

use crate::wire::protocol::ProtocolEvent;
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
    /// Creates an empty envelope.
    ///
    /// Equivalent to [`Envelope::default`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    #[must_use]
    pub fn with_node_id(mut self, id: impl Into<String>) -> Self {
        self.node_id = Some(id.into());
        self
    }

    #[must_use]
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

/// Envelope state for a single run stream.
///
/// Create one instance per run/session and call [`EnvelopeState::inject_into`]
/// for each outgoing event in stream order.
#[derive(Clone, Debug)]
pub struct EnvelopeState {
    pub session_id: String,
    pub current_node_id: String,
    pub node_run_seq: u64,
    pub next_event_id: u64,
}

impl EnvelopeState {
    /// Creates state for a new run/session.
    #[must_use]
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            current_node_id: String::new(),
            node_run_seq: 0,
            next_event_id: 1,
        }
    }

    fn active_node_id(&self) -> &str {
        if self.current_node_id.is_empty() {
            "run-0"
        } else {
            self.current_node_id.as_str()
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
        let env = Envelope::new()
            .with_session_id(&self.session_id)
            .with_node_id(self.active_node_id())
            .with_event_id(self.next_event_id);
        self.next_event_id += 1;
        env.inject_into(value);
    }

    /// Builds the envelope for the reply line (protocol_spec §5).
    ///
    /// This does not advance internal state; repeated calls return the same event_id
    /// until the next event is injected.
    #[must_use]
    pub fn reply_envelope(&self) -> Envelope {
        Envelope::new()
            .with_session_id(&self.session_id)
            .with_node_id(self.active_node_id())
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
    use serde_json::json;

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

    #[test]
    fn node_enter_advances_span_and_event_ids() {
        let mut state = EnvelopeState::new("sess-1".to_string());

        let first = to_json(
            &ProtocolEvent::NodeEnter {
                id: "think".to_string(),
            },
            &mut state,
        )
        .unwrap();
        let chunk = to_json(
            &ProtocolEvent::MessageChunk {
                content: "hello".to_string(),
                id: "think".to_string(),
            },
            &mut state,
        )
        .unwrap();
        let second = to_json(
            &ProtocolEvent::NodeEnter {
                id: "act".to_string(),
            },
            &mut state,
        )
        .unwrap();

        assert_eq!(first["node_id"], "run-think-0");
        assert_eq!(chunk["node_id"], "run-think-0");
        assert_eq!(second["node_id"], "run-act-1");
        assert_eq!(first["event_id"], 1);
        assert_eq!(chunk["event_id"], 2);
        assert_eq!(second["event_id"], 3);
    }

    #[test]
    fn reply_envelope_uses_next_event_id_without_advancing() {
        let mut state = EnvelopeState::new("sess-1".to_string());
        let mut event =
            json!({"type":"usage","prompt_tokens":1,"completion_tokens":1,"total_tokens":2});
        state.inject_into(&mut event);
        assert_eq!(event["event_id"], 1);

        let first_reply = state.reply_envelope();
        let second_reply = state.reply_envelope();
        assert_eq!(first_reply.event_id, Some(2));
        assert_eq!(second_reply.event_id, Some(2));
    }

    // ── Additional envelope coverage ──

    #[test]
    fn envelope_default_is_empty() {
        let env = Envelope::default();
        assert!(env.session_id.is_none());
        assert!(env.node_id.is_none());
        assert!(env.event_id.is_none());
    }

    #[test]
    fn envelope_new_is_default() {
        let env = Envelope::new();
        assert!(env.session_id.is_none());
        assert!(env.node_id.is_none());
        assert!(env.event_id.is_none());
    }

    #[test]
    fn envelope_builder_pattern() {
        let env = Envelope::new()
            .with_session_id("s-1")
            .with_node_id("n-1")
            .with_event_id(42);
        assert_eq!(env.session_id.as_deref(), Some("s-1"));
        assert_eq!(env.node_id.as_deref(), Some("n-1"));
        assert_eq!(env.event_id, Some(42));
    }

    #[test]
    fn envelope_inject_does_not_overwrite_existing_keys() {
        let mut obj = json!({"type":"node_enter","id":"think","session_id":"existing"});
        let env = Envelope::new()
            .with_session_id("new-session")
            .with_node_id("n-1")
            .with_event_id(1);
        env.inject_into(&mut obj);
        // session_id already present, should not be overwritten
        assert_eq!(obj["session_id"], "existing");
        assert_eq!(obj["node_id"], "n-1");
        assert_eq!(obj["event_id"], 1);
    }

    #[test]
    fn envelope_inject_into_non_object_is_noop() {
        let mut val = json!("not an object");
        let env = Envelope::new().with_session_id("s-1");
        env.inject_into(&mut val);
        // Still a string, not an object
        assert!(val.is_string());
    }

    #[test]
    fn envelope_inject_partial_fields() {
        let mut obj = json!({"type":"node_enter","id":"think"});
        let env = Envelope::new().with_session_id("s-1");
        // No node_id, no event_id
        env.inject_into(&mut obj);
        assert_eq!(obj["session_id"], "s-1");
        assert!(obj.get("node_id").is_none());
        assert!(obj.get("event_id").is_none());
    }

    #[test]
    fn envelope_state_active_node_id_default_is_run_0() {
        let state = EnvelopeState::new("s-1".to_string());
        let reply = state.reply_envelope();
        assert_eq!(reply.node_id.as_deref(), Some("run-0"));
    }

    #[test]
    fn envelope_state_event_ids_monotonically_increase() {
        let mut state = EnvelopeState::new("s-1".to_string());
        let mut ev1 = json!({"type":"usage"});
        let mut ev2 = json!({"type":"usage"});
        let mut ev3 = json!({"type":"usage"});
        state.inject_into(&mut ev1);
        state.inject_into(&mut ev2);
        state.inject_into(&mut ev3);
        assert_eq!(ev1["event_id"], 1);
        assert_eq!(ev2["event_id"], 2);
        assert_eq!(ev3["event_id"], 3);
    }

    #[test]
    fn envelope_state_node_enter_updates_current_node() {
        let mut state = EnvelopeState::new("s-1".to_string());

        // First node enter
        let mut enter_think = json!({"type":"node_enter","id":"think"});
        state.inject_into(&mut enter_think);
        assert_eq!(enter_think["node_id"], "run-think-0");
        assert_eq!(enter_think["session_id"], "s-1");

        // Message within that node
        let mut msg = json!({"type":"message_chunk","id":"think","content":"hi"});
        state.inject_into(&mut msg);
        assert_eq!(msg["node_id"], "run-think-0");

        // Second node enter
        let mut enter_act = json!({"type":"node_enter","id":"act"});
        state.inject_into(&mut enter_act);
        assert_eq!(enter_act["node_id"], "run-act-1");

        // Message within second node
        let mut msg2 = json!({"type":"message_chunk","id":"act","content":"done"});
        state.inject_into(&mut msg2);
        assert_eq!(msg2["node_id"], "run-act-1");
    }

    #[test]
    fn envelope_state_node_enter_with_empty_id() {
        let mut state = EnvelopeState::new("s-1".to_string());
        let mut enter = json!({"type":"node_enter","id":""});
        state.inject_into(&mut enter);
        // Empty id results in "run--0"
        assert_eq!(enter["node_id"], "run--0");
    }

    #[test]
    fn envelope_state_node_enter_without_id_field() {
        let mut state = EnvelopeState::new("s-1".to_string());
        let mut enter = json!({"type":"node_enter"});
        state.inject_into(&mut enter);
        // Missing id field defaults to empty, so "run--0"
        assert_eq!(enter["node_id"], "run--0");
    }

    #[test]
    fn to_json_with_non_node_enter_event() {
        let ev = ProtocolEvent::MessageChunk {
            content: "hello".to_string(),
            id: "think".to_string(),
        };
        let mut state = EnvelopeState::new("s-1".to_string());
        // Before any node_enter, active_node_id is "run-0"
        let val = to_json(&ev, &mut state).unwrap();
        assert_eq!(val["session_id"], "s-1");
        assert_eq!(val["node_id"], "run-0");
        assert_eq!(val["event_id"], 1);
    }

    #[test]
    fn envelope_state_reply_envelope_is_consistent() {
        let mut state = EnvelopeState::new("s-1".to_string());
        let mut ev = json!({"type":"node_enter","id":"think"});
        state.inject_into(&mut ev);

        let reply = state.reply_envelope();
        assert_eq!(reply.session_id.as_deref(), Some("s-1"));
        assert_eq!(reply.node_id.as_deref(), Some("run-think-0"));
        // reply_envelope returns next_event_id which is 2 after one inject
        assert_eq!(reply.event_id, Some(2));
    }
}
