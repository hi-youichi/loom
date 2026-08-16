//! Durable ACP session metadata stored beside Loom checkpoints.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionMetadata {
    pub session_id: String,
    pub thread_id: String,
    pub owner_principal: String,
    pub cwd: PathBuf,
    pub lifecycle: String,
    /// Auto-generated session title (first-turn LLM title), when known.
    pub title: Option<String>,
    /// RFC 3339 timestamp of the last metadata update.
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SessionRepository {
    db_path: PathBuf,
}

impl SessionRepository {
    pub fn new(db_path: impl Into<PathBuf>) -> rusqlite::Result<Self> {
        let repository = Self {
            db_path: db_path.into(),
        };
        repository.ensure_schema()?;
        Ok(repository)
    }

    fn connection(&self) -> rusqlite::Result<Connection> {
        Connection::open(&self.db_path)
    }

    fn ensure_schema(&self) -> rusqlite::Result<()> {
        self.connection()?.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS acp_sessions (
                session_id      TEXT PRIMARY KEY,
                thread_id       TEXT NOT NULL,
                owner_principal TEXT NOT NULL,
                cwd             TEXT NOT NULL,
                lifecycle       TEXT NOT NULL DEFAULT 'idle',
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL,
                closed_at       TEXT,
                title           TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_acp_sessions_owner_updated
                ON acp_sessions(owner_principal, updated_at DESC);
            "#,
        )?;
        self.ensure_title_column()?;
        Ok(())
    }

    /// Add the `title` column to databases created before it existed.
    fn ensure_title_column(&self) -> rusqlite::Result<()> {
        let has_title = self
            .connection()?
            .prepare("SELECT 1 FROM pragma_table_info('acp_sessions') WHERE name = 'title'")?
            .exists([])?;
        if !has_title {
            self.connection()?
                .execute_batch("ALTER TABLE acp_sessions ADD COLUMN title TEXT")?;
        }
        Ok(())
    }

    pub fn insert(
        &self,
        session_id: &str,
        thread_id: &str,
        owner_principal: &str,
        cwd: &Path,
    ) -> rusqlite::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.connection()?.execute(
            r#"
            INSERT INTO acp_sessions
                (session_id, thread_id, owner_principal, cwd, lifecycle, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, 'idle', ?5, ?5)
            "#,
            params![
                session_id,
                thread_id,
                owner_principal,
                cwd.to_string_lossy().as_ref(),
                now
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, session_id: &str) -> rusqlite::Result<Option<SessionMetadata>> {
        self.connection()?
            .query_row(
                r#"
                SELECT session_id, thread_id, owner_principal, cwd, lifecycle, title, updated_at
                FROM acp_sessions WHERE session_id = ?1
                "#,
                [session_id],
                |row| {
                    Ok(SessionMetadata {
                        session_id: row.get(0)?,
                        thread_id: row.get(1)?,
                        owner_principal: row.get(2)?,
                        cwd: PathBuf::from(row.get::<_, String>(3)?),
                        lifecycle: row.get(4)?,
                        title: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .optional()
    }

    pub fn list(&self) -> rusqlite::Result<Vec<SessionMetadata>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT session_id, thread_id, owner_principal, cwd, lifecycle, title, updated_at
            FROM acp_sessions ORDER BY updated_at ASC
            "#,
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok(SessionMetadata {
                    session_id: row.get(0)?,
                    thread_id: row.get(1)?,
                    owner_principal: row.get(2)?,
                    cwd: PathBuf::from(row.get::<_, String>(3)?),
                    lifecycle: row.get(4)?,
                    title: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })?
            .collect();
        rows
    }

    pub fn list_for_owner(&self, owner_principal: &str) -> rusqlite::Result<Vec<SessionMetadata>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT session_id, thread_id, owner_principal, cwd, lifecycle, title, updated_at
            FROM acp_sessions
            WHERE owner_principal = ?1
            ORDER BY updated_at DESC
            "#,
        )?;
        let rows = statement
            .query_map([owner_principal], |row| {
                Ok(SessionMetadata {
                    session_id: row.get(0)?,
                    thread_id: row.get(1)?,
                    owner_principal: row.get(2)?,
                    cwd: PathBuf::from(row.get::<_, String>(3)?),
                    lifecycle: row.get(4)?,
                    title: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })?
            .collect();
        rows
    }

    /// Persist an auto-generated session title. Does not bump `updated_at`
    /// so title generation never reorders the session list.
    pub fn set_title(&self, session_id: &str, title: &str) -> rusqlite::Result<()> {
        self.connection()?.execute(
            "UPDATE acp_sessions SET title = ?2 WHERE session_id = ?1",
            params![session_id, title],
        )?;
        Ok(())
    }

    pub fn set_lifecycle(&self, session_id: &str, lifecycle: &str) -> rusqlite::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let closed_at = (lifecycle == "closed").then_some(now.clone());
        self.connection()?.execute(
            r#"
            UPDATE acp_sessions
            SET lifecycle = ?2, updated_at = ?3, closed_at = ?4
            WHERE session_id = ?1
            "#,
            params![session_id, lifecycle, now, closed_at],
        )?;
        Ok(())
    }

    pub fn delete(&self, session_id: &str) -> rusqlite::Result<()> {
        self.connection()?.execute(
            "DELETE FROM acp_sessions WHERE session_id = ?1",
            [session_id],
        )?;
        Ok(())
    }

    /// Delete ACP metadata and all session-owned rows in one SQLite
    /// transaction. Optional tables are skipped when their feature has never
    /// initialized its schema.
    pub fn delete_all(&self, session_id: &str, thread_id: &str) -> rusqlite::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let table_exists = |name: &str| -> rusqlite::Result<bool> {
            transaction
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [name],
                    |_| Ok(()),
                )
                .optional()
                .map(|value| value.is_some())
        };

        if table_exists("review_status")? {
            transaction.execute(
                "DELETE FROM review_status WHERE session_id = ?1",
                [session_id],
            )?;
        }
        if table_exists("review_history")? {
            transaction.execute(
                "DELETE FROM review_history WHERE session_id = ?1",
                [session_id],
            )?;
        }
        if table_exists("session_config")? {
            transaction.execute(
                "DELETE FROM session_config WHERE session_id = ?1",
                [session_id],
            )?;
        }
        if table_exists("checkpoint_writes")? {
            transaction.execute(
                "DELETE FROM checkpoint_writes WHERE thread_id = ?1",
                [thread_id],
            )?;
        }
        if table_exists("checkpoints")? {
            transaction.execute("DELETE FROM checkpoints WHERE thread_id = ?1", [thread_id])?;
        }
        transaction.execute(
            "DELETE FROM acp_sessions WHERE session_id = ?1",
            [session_id],
        )?;
        transaction.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trip_and_delete_are_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let repository = SessionRepository::new(temp.path().join("sessions.db")).unwrap();
        repository
            .insert("session-a", "thread-a", "owner-a", temp.path())
            .unwrap();
        let metadata = repository.get("session-a").unwrap().unwrap();
        assert_eq!(metadata.owner_principal, "owner-a");
        assert_eq!(metadata.cwd, temp.path());

        repository.set_lifecycle("session-a", "closed").unwrap();
        assert_eq!(
            repository.get("session-a").unwrap().unwrap().lifecycle,
            "closed"
        );
        assert_eq!(repository.get("session-a").unwrap().unwrap().title, None);
        repository.set_title("session-a", "Fix login bug").unwrap();
        assert_eq!(
            repository.get("session-a").unwrap().unwrap().title,
            Some("Fix login bug".to_string())
        );
        repository.delete("session-a").unwrap();
        repository.delete("session-a").unwrap();
        assert!(repository.get("session-a").unwrap().is_none());
    }

    #[test]
    fn legacy_database_without_title_column_is_migrated() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("sessions.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE acp_sessions (
                    session_id      TEXT PRIMARY KEY,
                    thread_id       TEXT NOT NULL,
                    owner_principal TEXT NOT NULL,
                    cwd             TEXT NOT NULL,
                    lifecycle       TEXT NOT NULL DEFAULT 'idle',
                    created_at      TEXT NOT NULL,
                    updated_at      TEXT NOT NULL,
                    closed_at       TEXT
                );
                INSERT INTO acp_sessions
                    (session_id, thread_id, owner_principal, cwd, lifecycle, created_at, updated_at)
                VALUES
                    ('session-old', 'thread-old', 'owner-a', 'C:\\tmp', 'idle', 't0', 't0');
                "#,
            )
            .unwrap();
        }
        let repository = SessionRepository::new(&db_path).unwrap();
        let metadata = repository.get("session-old").unwrap().unwrap();
        assert_eq!(metadata.title, None);
        assert!(metadata.updated_at.is_some());
        repository.set_title("session-old", "Migrated").unwrap();
        assert_eq!(
            repository.get("session-old").unwrap().unwrap().title,
            Some("Migrated".to_string())
        );
    }

    #[test]
    fn delete_all_rolls_back_when_owned_table_delete_fails() {
        let temp = tempfile::tempdir().unwrap();
        let repository = SessionRepository::new(temp.path().join("sessions.db")).unwrap();
        repository
            .insert("session-a", "thread-a", "owner-a", temp.path())
            .unwrap();

        let connection = repository.connection().unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE review_status (
                    session_id TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                INSERT INTO review_status(session_id, value)
                    VALUES ('session-a', 'keep');
                CREATE TRIGGER fail_review_delete
                BEFORE DELETE ON review_status
                BEGIN
                    SELECT RAISE(ABORT, 'injected delete failure');
                END;
                "#,
            )
            .unwrap();

        assert!(repository.delete_all("session-a", "thread-a").is_err());
        assert!(repository.get("session-a").unwrap().is_some());
        assert_eq!(
            connection
                .query_row(
                    "SELECT value FROM review_status WHERE session_id = 'session-a'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "keep"
        );
    }
}
