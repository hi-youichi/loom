//! Persistent storage for session configuration (model, mode).
//!
//! Stores session config in a separate SQLite table alongside the checkpoint database.
//! This allows session configuration to survive Loom process restarts.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::session::SessionId;

/// Persistent store for session configuration key-value pairs.
///
/// Schema:
/// ```sql
/// CREATE TABLE IF NOT EXISTS session_config (
///     session_id TEXT NOT NULL,
///     key TEXT NOT NULL,
///     value TEXT NOT NULL,
///     updated_at INTEGER NOT NULL,
///     PRIMARY KEY (session_id, key)
/// );
/// ```
#[derive(Debug, Clone)]
pub struct SessionConfigStore {
    conn: Arc<Mutex<Connection>>,
}

impl SessionConfigStore {
    /// Open or create the session_config table in the given database.
    pub fn new(db_path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(db_path)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS session_config (
                session_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (session_id, key)
            );
            CREATE INDEX IF NOT EXISTS idx_session_config_session_id 
                ON session_config(session_id);",
        )?;
        Ok(())
    }

    /// Set a configuration value for a session.
    ///
    /// Uses INSERT OR REPLACE to upsert.
    pub fn set(&self, session_id: &SessionId, key: &str, value: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO session_config (session_id, key, value, updated_at) 
             VALUES (?1, ?2, ?3, strftime('%s', 'now'))",
            rusqlite::params![session_id.as_str(), key, value],
        )?;
        Ok(())
    }

    /// Get a single configuration value for a session.
    ///
    /// Returns `None` if the session or key doesn't exist.
    pub fn get(&self, session_id: &SessionId, key: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT value FROM session_config WHERE session_id = ?1 AND key = ?2"
        )?;
        let mut rows = stmt.query(rusqlite::params![session_id.as_str(), key])?;
        
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Get all configuration values for a session.
    pub fn get_all(&self, session_id: &SessionId) -> rusqlite::Result<HashMap<String, String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT key, value FROM session_config WHERE session_id = ?1"
        )?;
        let rows = stmt.query_map([session_id.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        
        let mut config = HashMap::new();
        for row in rows {
            let (key, value) = row?;
            config.insert(key, value);
        }
        Ok(config)
    }

    /// Delete all configuration for a session.
    ///
    /// Useful for cleanup when a session is removed.
    pub fn delete_session(&self, session_id: &SessionId) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM session_config WHERE session_id = ?1",
            [session_id.as_str()],
        )?;
        Ok(())
    }

    /// Copy all configuration from one session to another.
    ///
    /// Used by fork_session to duplicate config.
    pub fn copy_config(&self, from: &SessionId, to: &SessionId) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO session_config (session_id, key, value, updated_at)
             SELECT ?1, key, value, strftime('%s', 'now')
             FROM session_config WHERE session_id = ?2
             ON CONFLICT(session_id, key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at",
            rusqlite::params![to.as_str(), from.as_str()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_store() -> SessionConfigStore {
        // Use in-memory database for tests
        SessionConfigStore::new(":memory:").unwrap()
    }

    #[test]
    fn test_set_and_get() {
        let store = create_test_store();
        let session_id = SessionId::new("test-session-1");

        store.set(&session_id, "model", "gpt-4o").unwrap();
        store.set(&session_id, "mode", "code").unwrap();

        assert_eq!(store.get(&session_id, "model").unwrap(), Some("gpt-4o".to_string()));
        assert_eq!(store.get(&session_id, "mode").unwrap(), Some("code".to_string()));
        assert_eq!(store.get(&session_id, "nonexistent").unwrap(), None);
    }

    #[test]
    fn test_get_all() {
        let store = create_test_store();
        let session_id = SessionId::new("test-session-2");

        store.set(&session_id, "model", "claude-3-opus").unwrap();
        store.set(&session_id, "mode", "architect").unwrap();

        let all = store.get_all(&session_id).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("model").unwrap(), "claude-3-opus");
        assert_eq!(all.get("mode").unwrap(), "architect");
    }

    #[test]
    fn test_upsert() {
        let store = create_test_store();
        let session_id = SessionId::new("test-session-3");

        store.set(&session_id, "model", "gpt-4o").unwrap();
        store.set(&session_id, "model", "gpt-4o-mini").unwrap();

        assert_eq!(store.get(&session_id, "model").unwrap(), Some("gpt-4o-mini".to_string()));
    }

    #[test]
    fn test_copy_config() {
        let store = create_test_store();
        let source = SessionId::new("source-session");
        let target = SessionId::new("target-session");

        store.set(&source, "model", "gpt-4o").unwrap();
        store.set(&source, "mode", "ask").unwrap();

        store.copy_config(&source, &target).unwrap();

        let target_config = store.get_all(&target).unwrap();
        assert_eq!(target_config.len(), 2);
        assert_eq!(target_config.get("model").unwrap(), "gpt-4o");
        assert_eq!(target_config.get("mode").unwrap(), "ask");
    }

    #[test]
    fn test_delete_session() {
        let store = create_test_store();
        let session_id = SessionId::new("test-session-4");

        store.set(&session_id, "model", "gpt-4o").unwrap();
        store.delete_session(&session_id).unwrap();

        assert_eq!(store.get(&session_id, "model").unwrap(), None);
    }
}
