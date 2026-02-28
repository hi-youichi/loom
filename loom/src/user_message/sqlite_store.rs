//! SQLite-backed user message store. Persistent per-thread message history.

use std::path::Path;

use async_trait::async_trait;
use rusqlite::params;

use crate::message::Message;
use crate::user_message::{UserMessageStore, UserMessageStoreError};

/// SQLite-backed store: one table `user_messages (id, thread_id, role, content)`.
/// `id` is auto-increment and used as the pagination cursor (`before`).
pub struct SqliteUserMessageStore {
    db_path: std::path::PathBuf,
}

fn message_to_role_content(msg: &Message) -> (&'static str, &str) {
    match msg {
        Message::System(c) => ("system", c.as_str()),
        Message::User(c) => ("user", c.as_str()),
        Message::Assistant(c) => ("assistant", c.as_str()),
    }
}

fn row_to_message(role: &str, content: &str) -> Message {
    match role {
        "system" => Message::System(content.to_string()),
        "user" => Message::User(content.to_string()),
        "assistant" => Message::Assistant(content.to_string()),
        _ => Message::User(content.to_string()),
    }
}

impl SqliteUserMessageStore {
    /// Creates the store and ensures the table exists. `path` is the SQLite file path.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, UserMessageStoreError> {
        let db_path = path.as_ref().to_path_buf();
        let conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| UserMessageStoreError::Other(e.to_string()))?;
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS user_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL
            )
            "#,
            [],
        )
        .map_err(|e| UserMessageStoreError::Other(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_messages_thread_id ON user_messages(thread_id)",
            [],
        )
        .map_err(|e| UserMessageStoreError::Other(e.to_string()))?;
        Ok(Self { db_path })
    }
}

#[async_trait]
impl UserMessageStore for SqliteUserMessageStore {
    async fn append(
        &self,
        thread_id: &str,
        message: &Message,
    ) -> Result<(), UserMessageStoreError> {
        let (role, content) = message_to_role_content(message);
        let thread_id = thread_id.to_string();
        let content = content.to_string();
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| UserMessageStoreError::Other(e.to_string()))?;
            conn.execute(
                "INSERT INTO user_messages (thread_id, role, content) VALUES (?1, ?2, ?3)",
                params![thread_id, role, content],
            )
            .map_err(|e| UserMessageStoreError::Other(e.to_string()))?;
            Ok::<(), UserMessageStoreError>(())
        })
        .await
        .map_err(|e| UserMessageStoreError::Other(e.to_string()))?
    }

    async fn list(
        &self,
        thread_id: &str,
        before: Option<u64>,
        limit: Option<u32>,
    ) -> Result<Vec<Message>, UserMessageStoreError> {
        let thread_id = thread_id.to_string();
        let limit = limit.unwrap_or(100).min(1000);
        let db_path = self.db_path.clone();
        let rows: Vec<(String, String)> = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| UserMessageStoreError::Other(e.to_string()))?;
            let sql = match before {
                Some(_) => "SELECT role, content FROM user_messages WHERE thread_id = ?1 AND id < ?2 ORDER BY id ASC LIMIT ?3",
                None => "SELECT role, content FROM user_messages WHERE thread_id = ?1 ORDER BY id ASC LIMIT ?2",
            };
            let mut stmt = conn.prepare(sql).map_err(|e| UserMessageStoreError::Other(e.to_string()))?;
            let rows = match before {
                Some(b) => stmt.query(params![thread_id, b as i64, limit as i64]),
                None => stmt.query(params![thread_id, limit as i64]),
            }
            .map_err(|e| UserMessageStoreError::Other(e.to_string()))?;
            let mut out = Vec::new();
            let mut rows = rows;
            while let Some(row) = rows.next().map_err(|e| UserMessageStoreError::Other(e.to_string()))? {
                let role: String = row.get(0).map_err(|e| UserMessageStoreError::Other(e.to_string()))?;
                let content: String = row.get(1).map_err(|e| UserMessageStoreError::Other(e.to_string()))?;
                out.push((role, content));
            }
            Ok::<Vec<(String, String)>, UserMessageStoreError>(out)
        })
        .await
        .map_err(|e| UserMessageStoreError::Other(e.to_string()))??;
        Ok(rows
            .into_iter()
            .map(|(role, content)| row_to_message(&role, &content))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn sqlite_append_and_list_order() {
        let file = NamedTempFile::new().unwrap();
        let store = SqliteUserMessageStore::new(file.path()).unwrap();
        store.append("t1", &Message::user("hi")).await.unwrap();
        store.append("t1", &Message::assistant("hello")).await.unwrap();
        store.append("t1", &Message::user("bye")).await.unwrap();
        let msgs = store.list("t1", None, Some(10)).await.unwrap();
        assert_eq!(msgs.len(), 3);
        match &msgs[0] {
            Message::User(c) => assert_eq!(c, "hi"),
            _ => panic!("expected user"),
        }
        match &msgs[1] {
            Message::Assistant(c) => assert_eq!(c, "hello"),
            _ => panic!("expected assistant"),
        }
        match &msgs[2] {
            Message::User(c) => assert_eq!(c, "bye"),
            _ => panic!("expected user"),
        }
    }

    #[tokio::test]
    async fn sqlite_list_before_and_limit() {
        let file = NamedTempFile::new().unwrap();
        let store = SqliteUserMessageStore::new(file.path()).unwrap();
        for i in 0..5 {
            store.append("t2", &Message::user(format!("m{}", i))).await.unwrap();
        }
        let page1 = store.list("t2", None, Some(2)).await.unwrap();
        assert_eq!(page1.len(), 2);
        let id_before = 3u64; // cursor: next page starts before id 3
        let page2 = store.list("t2", Some(id_before), Some(2)).await.unwrap();
        assert_eq!(page2.len(), 2);
    }
}
