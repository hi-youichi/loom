//! SQLite-backed workspace store: workspaces and thread membership.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("storage: {0}")]
    Storage(String),
    #[error("not found: {0}")]
    NotFound(String),
}

/// Workspace metadata for list_workspaces.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceMeta {
    pub id: String,
    pub name: Option<String>,
    /// Milliseconds since Unix epoch.
    pub created_at_ms: i64,
}

/// Thread membership for list_threads (UI: "某 workspace 下所有对话列表").
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreadInWorkspace {
    pub thread_id: String,
    /// Optional display name (title) for this thread, typically auto-generated from the first message.
    pub name: Option<String>,
    /// Milliseconds since Unix epoch.
    pub created_at_ms: i64,
}

fn system_time_to_i64(t: SystemTime) -> i64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// SQLite-backed workspace store. Own DB, independent of loom checkpoint/store.
pub struct Store {
    db: Arc<Mutex<rusqlite::Connection>>,
}

impl Store {
    /// Opens or creates the database and tables.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        let conn =
            rusqlite::Connection::open(&path).map_err(|e| StoreError::Storage(e.to_string()))?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                name TEXT,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS workspace_threads (
                workspace_id TEXT NOT NULL,
                thread_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                name TEXT,
                PRIMARY KEY (workspace_id, thread_id),
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id)
            );
            CREATE INDEX IF NOT EXISTS idx_workspace_threads_workspace_id ON workspace_threads(workspace_id);
            "#,
        )
        .map_err(|e| StoreError::Storage(e.to_string()))?;
        // Migration: add name column to workspace_threads (idempotent, ignored if column already exists)
        match conn.execute_batch("ALTER TABLE workspace_threads ADD COLUMN name TEXT") {
            Ok(_) => {},
            Err(e) if e.to_string().contains("duplicate column") => {}
            Err(e) => {
                tracing::warn!("workspace_threads migration warning: {}", e);
            }
        }

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// Creates a workspace. Returns the id.
    pub async fn create_workspace(&self, name: Option<String>) -> Result<String, StoreError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = system_time_to_i64(SystemTime::now());
        let name = name.as_deref().map(String::from);
        let db = self.db.clone();
        tokio::task::block_in_place(|| {
            let conn = db.lock().map_err(|_| StoreError::Storage("lock".into()))?;
            conn.execute(
                "INSERT INTO workspaces (id, name, created_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![&id, name.as_deref(), now],
            )
            .map_err(|e| StoreError::Storage(e.to_string()))?;
            Ok(id)
        })
    }

    /// Lists all workspaces (no multi-tenant filter for now).
    pub async fn list_workspaces(&self) -> Result<Vec<WorkspaceMeta>, StoreError> {
        let db = self.db.clone();
        tokio::task::block_in_place(|| {
            let conn = db.lock().map_err(|_| StoreError::Storage("lock".into()))?;
            let mut stmt = conn
                .prepare("SELECT id, name, created_at FROM workspaces ORDER BY created_at ASC")
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    let created_at_ms: i64 = row.get(2)?;
                    Ok(WorkspaceMeta {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        created_at_ms,
                    })
                })
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| StoreError::Storage(e.to_string()))
        })
    }

    /// Lists threads in a workspace (for UI "某 workspace 下所有对话列表").
    pub async fn list_threads(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<ThreadInWorkspace>, StoreError> {
        let db = self.db.clone();
        let workspace_id = workspace_id.to_string();
        tokio::task::block_in_place(|| {
            let conn = db.lock().map_err(|_| StoreError::Storage("lock".into()))?;
            let mut stmt = conn
                .prepare(
                    "SELECT thread_id, name, created_at FROM workspace_threads WHERE workspace_id = ?1 ORDER BY created_at DESC",
                )
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params![workspace_id.as_str()], |row| {
                    Ok(ThreadInWorkspace {
                        thread_id: row.get(0)?,
                        name: row.get(1)?,
                        created_at_ms: row.get(2)?,
                    })
                })
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| StoreError::Storage(e.to_string()))
        })
    }

    /// Adds a thread to a workspace. Idempotent: existing row is a no-op.
    pub async fn add_thread_to_workspace(
        &self,
        workspace_id: &str,
        thread_id: &str,
    ) -> Result<(), StoreError> {
        let now = system_time_to_i64(SystemTime::now());
        let db = self.db.clone();
        let workspace_id = workspace_id.to_string();
        let thread_id = thread_id.to_string();
        tokio::task::block_in_place(|| {
            let conn = db.lock().map_err(|_| StoreError::Storage("lock".into()))?;
            conn.execute(
                "INSERT OR IGNORE INTO workspace_threads (workspace_id, thread_id, created_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![workspace_id, thread_id, now],
            )
            .map_err(|e| StoreError::Storage(e.to_string()))?;
            Ok(())
        })
    }

    /// Renames a workspace. Returns `NotFound` if the workspace does not exist.
    pub async fn rename_workspace(
        &self,
        workspace_id: &str,
        new_name: &str,
    ) -> Result<(), StoreError> {
        let db = self.db.clone();
        let workspace_id = workspace_id.to_string();
        let new_name = new_name.to_string();
        tokio::task::block_in_place(|| {
            let conn = db.lock().map_err(|_| StoreError::Storage("lock".into()))?;
            let rows = conn
                .execute(
                    "UPDATE workspaces SET name = ?1 WHERE id = ?2",
                    rusqlite::params![new_name, workspace_id],
                )
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            if rows == 0 {
                return Err(StoreError::NotFound(format!(
                    "workspace not found: {}",
                    workspace_id
                )));
            }
            Ok(())
        })
    }

    /// Removes a thread from a workspace.
    pub async fn remove_thread_from_workspace(
        &self,
        workspace_id: &str,
        thread_id: &str,
    ) -> Result<(), StoreError> {
        let db = self.db.clone();
        let workspace_id = workspace_id.to_string();
        let thread_id = thread_id.to_string();
        tokio::task::block_in_place(|| {
            let conn = db.lock().map_err(|_| StoreError::Storage("lock".into()))?;
            conn.execute(
                "DELETE FROM workspace_threads WHERE workspace_id = ?1 AND thread_id = ?2",
                rusqlite::params![workspace_id, thread_id],
            )
            .map_err(|e| StoreError::Storage(e.to_string()))?;
            Ok(())
        })
    }

    /// Sets or updates the display name (title) of a thread within a workspace.
    /// Returns `NotFound` if the thread is not a member of the workspace.
    pub async fn rename_thread(
        &self,
        workspace_id: &str,
        thread_id: &str,
        name: &str,
    ) -> Result<(), StoreError> {
        let db = self.db.clone();
        let workspace_id = workspace_id.to_string();
        let thread_id = thread_id.to_string();
        let name = name.to_string();
        tokio::task::block_in_place(|| {
            let conn = db.lock().map_err(|_| StoreError::Storage("lock".into()))?;
            let rows = conn
                .execute(
                    "UPDATE workspace_threads SET name = ?1 WHERE workspace_id = ?2 AND thread_id = ?3",
                    rusqlite::params![name, workspace_id, thread_id],
                )
                .map_err(|e| StoreError::Storage(e.to_string()))?;
            if rows == 0 {
                return Err(StoreError::NotFound(format!(
                    "thread {} not found in workspace {}",
                    thread_id, workspace_id
                )));
            }
            Ok(())
        })
    }
}
