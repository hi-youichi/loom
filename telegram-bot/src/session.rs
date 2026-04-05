use crate::error::BotError;
use crate::traits::SessionManager;
use async_trait::async_trait;
use rusqlite::Connection;
use std::sync::Mutex;

pub struct SqliteSessionManager {
    conn: Mutex<Connection>,
}

impl SqliteSessionManager {
    pub fn new() -> Result<Self, BotError> {
        let db_path = loom::memory::default_memory_db_path();
        let conn = Connection::open(&db_path)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

#[async_trait]
impl SessionManager for SqliteSessionManager {
    async fn reset(&self, thread_id: &str) -> Result<usize, BotError> {
        Ok(crate::download::reset_session(thread_id)?)
    }

    async fn exists(&self, thread_id: &str) -> Result<bool, BotError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM checkpoints WHERE thread_id = ?1",
            [thread_id],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }
}

impl Default for SqliteSessionManager {
    fn default() -> Self {
        Self::new().expect("Failed to open SQLite connection for session manager")
    }
}
