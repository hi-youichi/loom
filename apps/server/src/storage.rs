//! Persistence seam for loom-server (tasks LS-013b + LS-014).
//!
//! Defines a [`Store`] trait that abstracts load/save of sessions, messages,
//! parts, and recent events. The default [`InMemoryStore`] mirrors the
//! existing in-memory maps — it is honest (no silent no-ops) and sufficient
//! for this step. A future SQLite or JSON-file-backed impl can drop in by
//! implementing the same trait.
//!
//! Write-through is opt-in: `AppState::store` is `None` for tests (so they
//! stay isolated and behavior-identical) and `Some(InMemoryStore)` when
//! constructed via [`new_server_state_with_store`](crate::state::new_server_state_with_store).
//! When present, mutations in the handlers call the `persist_*` helpers in
//! `state.rs`, which forward to the store.
//!
//! Load-on-startup: [`load_from_store`](crate::state::load_from_store)
//! populates the in-memory maps from the store on construction. Guarded by
//! `store.is_some()` so existing tests (which pass `None`) are unaffected.

use std::collections::{HashMap, VecDeque};

use parking_lot::RwLock;

use crate::state::{GlobalEvent, MessageInfo, PartInfo, SessionInfo};
use crate::v2_event::V2Event;

/// Bounded event ring capacity — matches `state::EVENT_BUFFER_CAP`.
const STORE_EVENT_CAP: usize = 512;

/// Persistence boundary for durable session/message/part/event storage.
///
/// All methods take `&self` and use interior mutability so the trait is
/// object-safe behind `Arc<dyn Store + Send + Sync>`.
pub trait Store: Send + Sync {
    // ── write (write-through) ──

    /// Upsert a session by its `id`.
    fn save_session(&self, session: &SessionInfo);

    /// Remove a session (and cascades are the caller's responsibility).
    fn delete_session(&self, id: &str);

    /// Replace the entire message list for a session.
    fn save_messages(&self, session_id: &str, messages: &[MessageInfo]);

    /// Remove all messages for a session.
    fn delete_messages(&self, session_id: &str);

    /// Replace the entire part list for a message.
    fn save_parts(&self, message_id: &str, parts: &[PartInfo]);

    /// Remove all parts for a message.
    fn delete_parts(&self, message_id: &str);

    /// Append an event to the bounded ring (evicts oldest on overflow).
    fn push_event(&self, event: &GlobalEvent);
    fn append_v2_session_event(&self, event: &V2Event);
    fn delete_v2_session_events(&self, session_id: &str);

    // ── read (load-on-startup) ──

    fn load_sessions(&self) -> HashMap<String, SessionInfo>;
    fn load_messages(&self) -> HashMap<String, Vec<MessageInfo>>;
    fn load_parts(&self) -> HashMap<String, Vec<PartInfo>>;
    fn load_events(&self) -> VecDeque<GlobalEvent>;
    fn load_v2_session_events(&self) -> HashMap<String, VecDeque<V2Event>>;
}

/// Default in-memory implementation — mirrors the existing `AppState` maps.
///
/// Honest: every save/delete/load touches real data. No new dependencies.
#[derive(Default)]
pub struct InMemoryStore {
    sessions: RwLock<HashMap<String, SessionInfo>>,
    messages: RwLock<HashMap<String, Vec<MessageInfo>>>,
    parts: RwLock<HashMap<String, Vec<PartInfo>>>,
    events: RwLock<VecDeque<GlobalEvent>>,
    v2_events: RwLock<HashMap<String, VecDeque<V2Event>>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Store for InMemoryStore {
    fn save_session(&self, session: &SessionInfo) {
        self.sessions
            .write()
            .insert(session.id.clone(), session.clone());
    }

    fn delete_session(&self, id: &str) {
        self.sessions.write().remove(id);
    }

    fn save_messages(&self, session_id: &str, messages: &[MessageInfo]) {
        self.messages
            .write()
            .insert(session_id.to_string(), messages.to_vec());
    }

    fn delete_messages(&self, session_id: &str) {
        self.messages.write().remove(session_id);
    }

    fn save_parts(&self, message_id: &str, parts: &[PartInfo]) {
        self.parts
            .write()
            .insert(message_id.to_string(), parts.to_vec());
    }

    fn delete_parts(&self, message_id: &str) {
        self.parts.write().remove(message_id);
    }

    fn push_event(&self, event: &GlobalEvent) {
        let mut events = self.events.write();
        if events.len() >= STORE_EVENT_CAP {
            events.pop_front();
        }
        events.push_back(event.clone());
    }

    fn append_v2_session_event(&self, event: &V2Event) {
        let Some(session_id) = event.durable.as_ref().map(|d| d.aggregate_id.clone()) else {
            return;
        };
        let mut logs = self.v2_events.write();
        let log = logs.entry(session_id).or_default();
        if log.len() >= STORE_EVENT_CAP {
            log.pop_front();
        }
        log.push_back(event.clone());
    }

    fn delete_v2_session_events(&self, session_id: &str) {
        self.v2_events.write().remove(session_id);
    }

    fn load_sessions(&self) -> HashMap<String, SessionInfo> {
        self.sessions.read().clone()
    }

    fn load_messages(&self) -> HashMap<String, Vec<MessageInfo>> {
        self.messages.read().clone()
    }

    fn load_parts(&self) -> HashMap<String, Vec<PartInfo>> {
        self.parts.read().clone()
    }

    fn load_events(&self) -> VecDeque<GlobalEvent> {
        self.events.read().clone()
    }

    fn load_v2_session_events(&self) -> HashMap<String, VecDeque<V2Event>> {
        self.v2_events.read().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::GlobalEvent;

    #[test]
    fn in_memory_store_round_trips_session() {
        let store = InMemoryStore::new();
        let session = SessionInfo {
            id: "sess_test".to_string(),
            slug: "sess_test".to_string(),
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: "Test".to_string(),
            version: "0.1.0".to_string(),
            parent_id: None,
            workspace_id: None,
            path: None,
            summary: None,
            cost: None,
            tokens: None,
            share: None,
            permission: None,
            revert: None,
            extras: HashMap::new(),
            agent: None,
            model: None,
            time: crate::state::TimeInfo::default(),
            metadata: serde_json::json!({}),
        };
        store.save_session(&session);
        assert_eq!(store.load_sessions().len(), 1);

        let mut updated = session.clone();
        updated.title = "Updated".to_string();
        store.save_session(&updated);
        let loaded = store.load_sessions();
        assert_eq!(loaded.get("sess_test").unwrap().title, "Updated");

        store.delete_session("sess_test");
        assert!(store.load_sessions().is_empty());
    }

    #[test]
    fn in_memory_store_round_trips_messages_and_parts() {
        let store = InMemoryStore::new();
        let msg = MessageInfo {
            id: "msg_1".to_string(),
            session_id: "sess_1".to_string(),
            role: "user".to_string(),
            time: serde_json::json!({}),
            agent: "build".to_string(),
            model: None,
            parent_id: None,
            tool: None,
            finish: None,
            provider_id: None,
            model_id: None,
            path: None,
            cost: None,
            tokens: None,
            mode: None,
            ..Default::default()
        };
        store.save_messages("sess_1", &[msg.clone()]);
        assert_eq!(store.load_messages().get("sess_1").unwrap().len(), 1);

        let part = PartInfo {
            id: "prt_1".to_string(),
            session_id: "sess_1".to_string(),
            message_id: "msg_1".to_string(),
            part_type: "text".to_string(),
            data: serde_json::json!({"text": "hello"}),
        };
        store.save_parts("msg_1", &[part]);
        assert_eq!(store.load_parts().get("msg_1").unwrap().len(), 1);

        store.delete_messages("sess_1");
        store.delete_parts("msg_1");
        assert!(store.load_messages().is_empty());
        assert!(store.load_parts().is_empty());
    }

    #[test]
    fn in_memory_store_bounded_event_ring() {
        let store = InMemoryStore::new();
        for _ in 0..(STORE_EVENT_CAP + 10) {
            store.push_event(&GlobalEvent::new(
                "/tmp".to_string(),
                None,
                None,
                "test.event".to_string(),
                serde_json::json!({}),
            ));
        }
        assert_eq!(store.load_events().len(), STORE_EVENT_CAP);
    }
}
