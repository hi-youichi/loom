//! Atomic session-to-connection ownership indexes.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use crate::connection::ConnectionId;
use crate::session::SessionId;

#[derive(Debug, Default)]
struct BindingState {
    session_to_connections: HashMap<SessionId, HashSet<ConnectionId>>,
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

    /// Bind a newly-created session to a connection. Returns true if this is a new binding.
    pub fn bind_new_session(&self, session_id: SessionId, connection_id: ConnectionId) -> bool {
        self.add_connection_to_session(&session_id, connection_id)
    }

    /// Add a connection to a session while updating both indexes under one
    /// write lock. Returns true if the connection was newly added.
    pub fn add_connection_to_session(
        &self,
        session_id: &SessionId,
        connection_id: ConnectionId,
    ) -> bool {
        let mut state = self.inner.write().unwrap_or_else(|e| e.into_inner());

        // Add connection to session's connection set
        let is_new = state
            .session_to_connections
            .entry(session_id.clone())
            .or_default()
            .insert(connection_id.clone());

        // Add session to connection's session set
        state
            .connection_to_sessions
            .entry(connection_id)
            .or_default()
            .insert(session_id.clone());

        is_new
    }

    /// Remove a connection from a session. Returns true if the connection was bound.
    pub fn remove_connection_from_session(
        &self,
        session_id: &SessionId,
        connection_id: &ConnectionId,
    ) -> bool {
        let mut state = self.inner.write().unwrap_or_else(|e| e.into_inner());

        // Remove connection from session's connection set
        let was_bound = state
            .session_to_connections
            .get_mut(session_id)
            .map(|conns| conns.remove(connection_id))
            .unwrap_or(false);

        // Clean up empty session entry
        if state
            .session_to_connections
            .get(session_id)
            .map(|conns| conns.is_empty())
            .unwrap_or(false)
        {
            state.session_to_connections.remove(session_id);
        }

        // Remove session from connection's session set
        if let Some(sessions) = state.connection_to_sessions.get_mut(connection_id) {
            sessions.remove(session_id);
            if sessions.is_empty() {
                state.connection_to_sessions.remove(connection_id);
            }
        }

        was_bound
    }

    /// Check if a session is bound to any connection.
    pub fn is_session_bound(&self, session_id: &SessionId) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .session_to_connections
            .get(session_id)
            .map(|conns| !conns.is_empty())
            .unwrap_or(false)
    }

    /// Check if a specific connection is bound to a session.
    pub fn is_connection_bound_to_session(
        &self,
        session_id: &SessionId,
        connection_id: &ConnectionId,
    ) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .session_to_connections
            .get(session_id)
            .map(|conns| conns.contains(connection_id))
            .unwrap_or(false)
    }

    /// Get all connections bound to a session.
    pub fn connections_for(&self, session_id: &SessionId) -> Vec<ConnectionId> {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .session_to_connections
            .get(session_id)
            .map(|conns| conns.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Legacy method for backward compatibility - returns the first connection or none.
    pub fn connection_for(&self, session_id: &SessionId) -> Option<ConnectionId> {
        self.connections_for(session_id).into_iter().next()
    }

    /// Get all sessions for a connection.
    pub fn sessions_for(&self, connection_id: &str) -> Vec<SessionId> {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .connection_to_sessions
            .get(connection_id)
            .map(|sessions| sessions.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Remove a session from all connections. Returns connections that were bound.
    pub fn unbind_session(&self, session_id: &SessionId) -> Vec<ConnectionId> {
        let mut state = self.inner.write().unwrap_or_else(|e| e.into_inner());

        let connections = state
            .session_to_connections
            .remove(session_id)
            .unwrap_or_default();

        // Remove this session from all connection sets
        for connection_id in &connections {
            if let Some(sessions) = state.connection_to_sessions.get_mut(connection_id) {
                sessions.remove(session_id);
                if sessions.is_empty() {
                    state.connection_to_sessions.remove(connection_id);
                }
            }
        }

        connections.into_iter().collect()
    }

    /// Remove all bindings owned by a disconnected transport.
    pub fn unbind_connection(&self, connection_id: &str) -> Vec<SessionId> {
        let mut state = self.inner.write().unwrap_or_else(|e| e.into_inner());
        let sessions = state
            .connection_to_sessions
            .remove(connection_id)
            .unwrap_or_default();

        // Remove this connection from all session sets
        for session_id in &sessions {
            if let Some(connections) = state.session_to_connections.get_mut(session_id) {
                connections.remove(connection_id);
                if connections.is_empty() {
                    state.session_to_connections.remove(session_id);
                }
            }
        }

        sessions.into_iter().collect()
    }

    /// Legacy method for backward compatibility - rebind to single connection.
    pub fn rebind_session(
        &self,
        session_id: &SessionId,
        connection_id: ConnectionId,
    ) -> Option<ConnectionId> {
        // For backward compatibility, remove all existing connections and add the new one
        let previous_connections = self.unbind_session(session_id);
        self.add_connection_to_session(session_id, connection_id);
        previous_connections.into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_multi_connection_binding() {
        let bindings = SessionBindings::new();
        let session = SessionId::new("session-a");

        // Bind first connection
        assert!(bindings.bind_new_session(session.clone(), "connection-a".into()));
        assert_eq!(
            bindings.connections_for(&session),
            vec!["connection-a".to_string()]
        );

        // Bind second connection - should succeed
        assert!(bindings.add_connection_to_session(&session, "connection-b".into()));

        let conns = bindings.connections_for(&session);
        assert_eq!(conns.len(), 2);
        assert!(conns.contains(&"connection-a".into()));
        assert!(conns.contains(&"connection-b".into()));
    }

    #[test]
    fn test_connection_bound_check() {
        let bindings = SessionBindings::new();
        let session = SessionId::new("session-a");

        bindings.add_connection_to_session(&session, "connection-a".into());

        assert!(bindings.is_connection_bound_to_session(&session, &"connection-a".into()));
        assert!(!bindings.is_connection_bound_to_session(&session, &"connection-b".into()));
    }

    #[test]
    fn test_remove_connection_from_session() {
        let bindings = SessionBindings::new();
        let session = SessionId::new("session-a");

        bindings.add_connection_to_session(&session, "connection-a".into());
        bindings.add_connection_to_session(&session, "connection-b".into());

        // Remove one connection
        assert!(bindings.remove_connection_from_session(&session, &"connection-a".into()));

        let conns = bindings.connections_for(&session);
        assert_eq!(conns, vec!["connection-b".to_string()]);

        // Session should still be bound
        assert!(bindings.is_session_bound(&session));
    }

    #[test]
    fn test_unbind_session_removes_all_connections() {
        let bindings = SessionBindings::new();
        let session = SessionId::new("session-a");

        bindings.add_connection_to_session(&session, "connection-a".into());
        bindings.add_connection_to_session(&session, "connection-b".into());

        let removed = bindings.unbind_session(&session);
        assert_eq!(removed.len(), 2);
        assert!(!bindings.is_session_bound(&session));
        assert!(bindings.connections_for(&session).is_empty());
    }

    #[test]
    fn test_unbind_connection() {
        let bindings = SessionBindings::new();
        let session_a = SessionId::new("session-a");
        let session_b = SessionId::new("session-b");

        bindings.add_connection_to_session(&session_a, "connection-a".into());
        bindings.add_connection_to_session(&session_b, "connection-a".into());

        let removed = bindings.unbind_connection("connection-a");
        assert_eq!(removed.len(), 2);
        assert!(!bindings.is_session_bound(&session_a));
        assert!(!bindings.is_session_bound(&session_b));
    }

    #[test]
    fn test_legacy_compatibility() {
        let bindings = SessionBindings::new();
        let session = SessionId::new("session-a");

        // Test legacy bind_new_session behavior
        bindings.bind_new_session(session.clone(), "connection-a".into());
        assert_eq!(
            bindings.connection_for(&session),
            Some("connection-a".into())
        );

        // Test legacy rebind_session behavior
        let prev = bindings.rebind_session(&session, "connection-b".into());
        assert_eq!(prev, Some("connection-a".into()));
        assert_eq!(
            bindings.connection_for(&session),
            Some("connection-b".into())
        );
        assert!(bindings.connections_for(&session).len() == 1); // Should only have one after rebind
    }

    #[test]
    fn test_concurrent_multi_connection() {
        let bindings = Arc::new(SessionBindings::new());
        let session = SessionId::new("session-a");
        let mut threads = Vec::new();

        for index in 0..4 {
            let bindings = bindings.clone();
            let session = session.clone();
            threads.push(std::thread::spawn(move || {
                bindings.add_connection_to_session(&session, format!("connection-{index}"));
            }));
        }

        for thread in threads {
            thread.join().unwrap();
        }

        // All connections should be bound
        let conns = bindings.connections_for(&session);
        assert_eq!(conns.len(), 4);

        // Each connection should see the session
        for index in 0..4 {
            assert!(bindings
                .sessions_for(&format!("connection-{index}"))
                .contains(&session));
        }
    }
}
