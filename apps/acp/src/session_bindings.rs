//! Atomic session-to-connection ownership indexes.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use crate::connection::ConnectionId;
use crate::session::SessionId;

#[derive(Debug, Default)]
struct BindingState {
    session_to_connection: HashMap<SessionId, ConnectionId>,
    connection_to_sessions: HashMap<ConnectionId, HashSet<SessionId>>,
}

/// The sole source of truth for transient session connection ownership.
#[derive(Debug, Default)]
pub struct SessionBindings {
    inner: RwLock<BindingState>,
}

impl SessionBindings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind a newly-created session. Returns the previous connection when the
    /// caller accidentally reuses an existing session id.
    pub fn bind_new_session(
        &self,
        session_id: SessionId,
        connection_id: ConnectionId,
    ) -> Option<ConnectionId> {
        self.rebind_session(&session_id, connection_id)
    }

    /// Move one session to a connection while updating both indexes under one
    /// write lock.
    pub fn rebind_session(
        &self,
        session_id: &SessionId,
        connection_id: ConnectionId,
    ) -> Option<ConnectionId> {
        let mut state = self.inner.write().unwrap_or_else(|e| e.into_inner());
        let previous = state
            .session_to_connection
            .insert(session_id.clone(), connection_id.clone());

        if let Some(previous_id) = previous.as_ref() {
            if previous_id != &connection_id {
                if let Some(sessions) = state.connection_to_sessions.get_mut(previous_id) {
                    sessions.remove(session_id);
                    if sessions.is_empty() {
                        state.connection_to_sessions.remove(previous_id);
                    }
                }
            }
        }

        state
            .connection_to_sessions
            .entry(connection_id)
            .or_default()
            .insert(session_id.clone());
        previous
    }

    pub fn connection_for(&self, session_id: &SessionId) -> Option<ConnectionId> {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .session_to_connection
            .get(session_id)
            .cloned()
    }

    pub fn sessions_for(&self, connection_id: &str) -> Vec<SessionId> {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .connection_to_sessions
            .get(connection_id)
            .map(|sessions| sessions.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn unbind_session(&self, session_id: &SessionId) -> Option<ConnectionId> {
        let mut state = self.inner.write().unwrap_or_else(|e| e.into_inner());
        let previous = state.session_to_connection.remove(session_id)?;
        if let Some(sessions) = state.connection_to_sessions.get_mut(&previous) {
            sessions.remove(session_id);
            if sessions.is_empty() {
                state.connection_to_sessions.remove(&previous);
            }
        }
        Some(previous)
    }

    /// Remove all bindings owned by a disconnected transport.
    pub fn unbind_connection(&self, connection_id: &str) -> Vec<SessionId> {
        let mut state = self.inner.write().unwrap_or_else(|e| e.into_inner());
        let sessions = state
            .connection_to_sessions
            .remove(connection_id)
            .unwrap_or_default();
        for session_id in &sessions {
            state.session_to_connection.remove(session_id);
        }
        sessions.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn rebind_updates_both_indexes() {
        let bindings = SessionBindings::new();
        let session = SessionId::new("session-a");
        bindings.bind_new_session(session.clone(), "connection-a".into());

        assert_eq!(
            bindings.connection_for(&session).as_deref(),
            Some("connection-a")
        );
        assert_eq!(bindings.sessions_for("connection-a"), vec![session.clone()]);

        assert_eq!(
            bindings
                .rebind_session(&session, "connection-b".into())
                .as_deref(),
            Some("connection-a")
        );
        assert!(bindings.sessions_for("connection-a").is_empty());
        assert_eq!(bindings.sessions_for("connection-b"), vec![session.clone()]);
    }

    #[test]
    fn unbind_connection_removes_forward_entries() {
        let bindings = SessionBindings::new();
        let a = SessionId::new("session-a");
        let b = SessionId::new("session-b");
        bindings.bind_new_session(a.clone(), "connection-a".into());
        bindings.bind_new_session(b.clone(), "connection-a".into());

        let mut removed = bindings.unbind_connection("connection-a");
        removed.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        assert_eq!(removed, vec![a.clone(), b.clone()]);
        assert!(bindings.connection_for(&a).is_none());
        assert!(bindings.connection_for(&b).is_none());
    }

    #[test]
    fn concurrent_rebind_leaves_exactly_one_owner() {
        let bindings = Arc::new(SessionBindings::new());
        let session = SessionId::new("session-a");
        let mut threads = Vec::new();
        for index in 0..8 {
            let bindings = bindings.clone();
            let session = session.clone();
            threads.push(std::thread::spawn(move || {
                bindings.rebind_session(&session, format!("connection-{index}"));
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }

        let winner = bindings.connection_for(&session).expect("one owner");
        let owner_count = (0..8)
            .filter(|index| {
                bindings
                    .sessions_for(&format!("connection-{index}"))
                    .contains(&session)
            })
            .count();
        assert_eq!(owner_count, 1);
        assert!(bindings.sessions_for(&winner).contains(&session));
    }
}
