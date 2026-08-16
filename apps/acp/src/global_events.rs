//! Global cross-connection event bus.
//!
//! Replaces the Express SSE global-event streams (`/api/global/event`,
//! `/api/notifications/stream`) with ACP JSON-RPC notifications
//! (`_loomdesk.dev/global/update`) pushed over each subscribed connection.
//!
//! Topics are coarse-grained change signals (`session`, `settings`, `git`,
//! `notification`); payloads are opencode-shaped events
//! (`{ type, properties }`) so the frontend event pipeline can reuse its
//! existing reducers unchanged.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::connection::ConnectionOutbound;
use crate::connection_registry::ConnectionRegistry;

/// JSON-RPC notification method used for all global events.
pub const GLOBAL_UPDATE_METHOD: &str = "_loomdesk.dev/global/update";

/// Known topics. Subscriptions may also use `"*"` for everything.
pub const TOPICS: &[&str] = &["session", "settings", "git", "notification"];

#[derive(Debug, thiserror::Error)]
pub enum GlobalEventError {
    #[error("unknown topic: {0}")]
    UnknownTopic(String),
    #[error("unknown connection: {0}")]
    UnknownConnection(String),
}

#[derive(Debug, Default)]
pub struct GlobalEventBus {
    connections: Mutex<Option<Arc<ConnectionRegistry>>>,
    subscriptions: Mutex<HashMap<String, HashSet<String>>>,
}

impl GlobalEventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind the connection registry. Called once by the runtime after the
    /// registry exists; handlers constructed before that retain the bus and
    /// publish lazily.
    pub fn bind_registry(&self, registry: Arc<ConnectionRegistry>) {
        *self.connections.lock().expect("global bus poisoned") = Some(registry);
    }

    /// Publish an opencode-shaped event (`{ type, properties }`) to every
    /// active subscription of `topic`.
    ///
    /// Fire-and-forget: slow consumers drop frames (events are change
    /// signals; consumers re-fetch authoritative state).
    pub fn publish(&self, topic: &str, event_type: &str, properties: Value) {
        let registry = match self.connections.lock().expect("global bus poisoned").clone() {
            Some(registry) => registry,
            None => return,
        };
        let params = json!({
            "topic": topic,
            "event": { "type": event_type, "properties": properties },
        });
        let subscribers = self.subscriptions.lock().expect("global bus poisoned");
        for (connection_id, topics) in subscribers.iter() {
            if !topics.contains(topic) && !topics.contains("*") {
                continue;
            }
            let Some(connection) = registry.get(connection_id) else {
                continue;
            };
            if !connection.is_active() || !connection.is_initialized() {
                continue;
            }
            let outbound = ConnectionOutbound::GlobalNotification {
                method: GLOBAL_UPDATE_METHOD.to_string(),
                params: params.clone(),
            };
            if let Err(error) = connection.outbound_tx.try_send(outbound) {
                tracing::debug!(
                    connection = %connection_id,
                    %error,
                    "global event dropped (outbound queue full or closed)"
                );
            }
        }
    }

    /// Replace the topic set for a connection. Validates topics; `"*"` is
    /// allowed as "everything".
    pub fn subscribe(
        &self,
        connection_id: &str,
        topics: &[String],
    ) -> Result<Vec<String>, GlobalEventError> {
        let mut normalized = HashSet::new();
        for topic in topics {
            if topic == "*" {
                normalized.insert("*".to_string());
                continue;
            }
            if !TOPICS.contains(&topic.as_str()) {
                return Err(GlobalEventError::UnknownTopic(topic.clone()));
            }
            normalized.insert(topic.clone());
        }
        let list: Vec<String> = normalized.iter().cloned().collect();
        self.subscriptions
            .lock()
            .expect("global bus poisoned")
            .insert(connection_id.to_string(), normalized);
        Ok(list)
    }

    /// Remove a connection's subscription entirely.
    pub fn unsubscribe(&self, connection_id: &str) {
        self.subscriptions
            .lock()
            .expect("global bus poisoned")
            .remove(connection_id);
    }

    /// Current topics for a connection (empty when not subscribed).
    pub fn topics_for(&self, connection_id: &str) -> Vec<String> {
        self.subscriptions
            .lock()
            .expect("global bus poisoned")
            .get(connection_id)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribe_rejects_unknown_topics() {
        let bus = GlobalEventBus::new();
        assert!(matches!(
            bus.subscribe("c1", &["nope".into()]),
            Err(GlobalEventError::UnknownTopic(_))
        ));
        assert!(bus.subscribe("c1", &["session".into()]).is_ok());
    }

    #[test]
    fn subscribe_star_covers_everything() {
        let bus = GlobalEventBus::new();
        bus.subscribe("c1", &["*".into()]).unwrap();
        assert!(bus.topics_for("c1").contains(&"*".to_string()));
    }

    #[test]
    fn unsubscribe_removes_state() {
        let bus = GlobalEventBus::new();
        bus.subscribe("c1", &["git".into()]).unwrap();
        bus.unsubscribe("c1");
        assert!(bus.topics_for("c1").is_empty());
    }

    #[test]
    fn publish_without_registry_is_noop() {
        let bus = GlobalEventBus::new();
        bus.subscribe("c1", &["*".into()]).unwrap();
        bus.publish("session", "session.updated", serde_json::json!({}));
    }
}
