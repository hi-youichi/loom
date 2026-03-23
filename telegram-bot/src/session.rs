//! Session management implementations

use async_trait::async_trait;
use crate::traits::SessionManager;
use crate::error::BotError;

pub struct SqliteSessionManager;

impl SqliteSessionManager {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SessionManager for SqliteSessionManager {
    async fn reset(&self, thread_id: &str) -> Result<usize, BotError> {
        crate::download::reset_session(thread_id)
            .map_err(|e| BotError::Database(e.to_string()))
    }

    async fn exists(&self, thread_id: &str) -> Result<bool, BotError> {
        let db_path = loom::memory::default_memory_db_path();
        
        let conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| BotError::Database(e.to_string()))?;
        
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM checkpoints WHERE thread_id = ?1",
                [thread_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        
        Ok(count > 0)
    }
}

impl Default for SqliteSessionManager {
    fn default() -> Self {
        Self::new()
    }
}
