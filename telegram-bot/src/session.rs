use std::sync::Mutex;
use rusqlite::Connection;
use crate::traits::SessionManager;
use crate::error::BotError;
use async_trait::async_trait;

pub struct SqliteSessionManager {
    conn: Mutex<Connection>,
}

impl SqliteSessionManager {
    pub fn new() -> Result<Self, BotError> {
        let db_path = loom::memory::default_memory_db_path();
        let conn = Connection::open(&db_path)
            .map_err(|e| BotError::Database(e.to_string()))?;
        Ok(Self { conn: Mutex::new(conn) })
    }
}

#[async_trait]
impl SessionManager for SqliteSessionManager {
    async fn reset(&self, thread_id: &str) -> Result<usize, BotError> {
        crate::download::reset_session(thread_id)
            .map_err(|e| BotError::Database(e.to_string()))
    }

    async fn exists(&self, thread_id: &str) -> Result<bool, BotError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM checkpoints WHERE thread_id = ?1",
                [thread_id],
                |row| row.get(0),
            )
            .map_err(|e| BotError::Database(e.to_string()))?;

        Ok(count > 0)
    }
}

impl Default for SqliteSessionManager {
    fn default() -> Self {
        Self::new().expect("Failed to open SQLite connection for session manager")
    }
}
