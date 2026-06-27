//! Review history persistence (SQLite).

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A single review execution record (append-only audit log).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRecord {
    pub session_id: String,
    pub reviewed_at: DateTime<Utc>,
    pub trigger: String,
    pub model: String,
    pub memory_update_count: usize,
    pub skill_update_count: usize,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub duration_ms: u64,
}

/// Aggregated review status for a session, derived from the latest record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewStatus {
    Reviewed,
    Skipped,
    Pending,
}

impl ReviewStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewStatus::Reviewed => "reviewed",
            ReviewStatus::Skipped => "skipped",
            ReviewStatus::Pending => "pending",
        }
    }

    pub fn from_skipped(skipped: bool) -> Self {
        if skipped {
            ReviewStatus::Skipped
        } else {
            ReviewStatus::Reviewed
        }
    }
}

/// SQLite-backed review history stored in `memory.db`.
///
/// Two tables:
/// - `review_history` — append-only audit log (all records)
/// - `review_status`  — per-session upsert (latest status only)
pub struct ReviewHistory {
    db_path: PathBuf,
}

impl ReviewHistory {
    /// Creates a handle backed by `<loom_home>/memory.db`.
    pub fn new(loom_home: &Path) -> Self {
        let db_path = loom_home.join("memory.db");
        Self { db_path }
    }

    /// Creates a handle backed by an explicit db path (for tests).
    pub fn with_db_path(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    fn open(&self) -> Result<Connection, String> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create db dir: {}", e))?;
        }
        let conn = loom_memory::sqlite_util::open_sqlite_with_wal(&self.db_path)?;
        self.init_schema(&conn)?;
        self.migrate_from_jsonl_if_needed(&conn)?;
        Ok(conn)
    }

    /// One-time migration: if the legacy `history.jsonl` exists and
    /// `history.jsonl.bak` does NOT (migration not yet done), import all
    /// records then rename the file to `.bak`.
    fn migrate_from_jsonl_if_needed(&self, conn: &Connection) -> Result<(), String> {
        let Some(loom_home) = self.db_path.parent() else {
            return Ok(());
        };
        let jsonl_path = loom_home.join("data").join("review").join("history.jsonl");
        let bak_path = loom_home.join("data").join("review").join("history.jsonl.bak");

        // Already migrated (.bak exists) or nothing to migrate (.jsonl absent)
        if bak_path.exists() || !jsonl_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&jsonl_path)
            .map_err(|e| format!("Migration: failed to read {}: {}", jsonl_path.display(), e))?;

        let mut imported = 0usize;
        for (lineno, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let record: ReviewRecord = serde_json::from_str(line).map_err(|e| {
                format!(
                    "Migration: failed to parse {} line {}: {}",
                    jsonl_path.display(),
                    lineno + 1,
                    e
                )
            })?;
            self.append_record_in_tx(conn, &record)?;
            imported += 1;
        }

        let bak = jsonl_path.with_extension("jsonl.bak");
        let _ = std::fs::rename(&jsonl_path, &bak);
        eprintln!(
            "Review history: migrated {} records from JSONL to SQLite",
            imported
        );
        Ok(())
    }

    /// Inserts a record into both tables using an existing connection (no new open).
    fn append_record_in_tx(&self, conn: &Connection, record: &ReviewRecord) -> Result<(), String> {
        conn.execute(
            "INSERT INTO review_history
                (session_id, reviewed_at, trigger, model,
                 memory_update_count, skill_update_count,
                 skipped, skip_reason, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                record.session_id,
                record.reviewed_at.timestamp_millis(),
                record.trigger,
                record.model,
                record.memory_update_count as i64,
                record.skill_update_count as i64,
                record.skipped as i64,
                record.skip_reason,
                record.duration_ms as i64,
            ],
        )
        .map_err(|e| format!("Migration insert review_history: {}", e))?;

        let history_id = conn.last_insert_rowid();
        let status = ReviewStatus::from_skipped(record.skipped).as_str();

        conn.execute(
            "INSERT INTO review_status
                (session_id, status, reviewed_at, trigger, model,
                 memory_update_count, skill_update_count,
                 skip_reason, duration_ms, history_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(session_id) DO UPDATE SET
                status = excluded.status,
                reviewed_at = excluded.reviewed_at,
                trigger = excluded.trigger,
                model = excluded.model,
                memory_update_count = excluded.memory_update_count,
                skill_update_count = excluded.skill_update_count,
                skip_reason = excluded.skip_reason,
                duration_ms = excluded.duration_ms,
                history_id = excluded.history_id",
            params![
                record.session_id,
                status,
                record.reviewed_at.timestamp_millis(),
                record.trigger,
                record.model,
                record.memory_update_count as i64,
                record.skill_update_count as i64,
                record.skip_reason,
                record.duration_ms as i64,
                history_id,
            ],
        )
        .map_err(|e| format!("Migration upsert review_status: {}", e))?;
        Ok(())
    }

    fn init_schema(&self, conn: &Connection) -> Result<(), String> {
        Self::ensure_schema(conn)
    }

    /// Ensures the review tables exist in the given connection (idempotent).
    ///
    /// Callers that open `memory.db` directly — e.g. to LEFT JOIN against
    /// `review_status` from the ACP session list — should invoke this before
    /// querying so a database with no review ever recorded does not yield
    /// "no such table" errors.
    pub fn ensure_schema(conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS review_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                reviewed_at INTEGER NOT NULL,
                trigger TEXT NOT NULL,
                model TEXT NOT NULL,
                memory_update_count INTEGER NOT NULL,
                skill_update_count INTEGER NOT NULL,
                skipped INTEGER NOT NULL,
                skip_reason TEXT,
                duration_ms INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_review_history_session
                ON review_history(session_id);
            CREATE INDEX IF NOT EXISTS idx_review_history_id_desc
                ON review_history(id DESC);

            CREATE TABLE IF NOT EXISTS review_status (
                session_id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                reviewed_at INTEGER NOT NULL,
                trigger TEXT NOT NULL,
                model TEXT NOT NULL,
                memory_update_count INTEGER NOT NULL,
                skill_update_count INTEGER NOT NULL,
                skip_reason TEXT,
                duration_ms INTEGER NOT NULL,
                history_id INTEGER NOT NULL REFERENCES review_history(id)
            );",
        )
        .map_err(|e| format!("Failed to init review schema: {}", e))?;
        Ok(())
    }

    /// Appends a record in a single transaction, updating both tables.
    pub fn append(&self, record: &ReviewRecord) -> Result<(), String> {
        let mut conn = self.open()?;
        let tx = conn.transaction().map_err(|e| format!("Begin tx: {}", e))?;

        tx.execute(
            "INSERT INTO review_history
                (session_id, reviewed_at, trigger, model,
                 memory_update_count, skill_update_count,
                 skipped, skip_reason, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                record.session_id,
                record.reviewed_at.timestamp_millis(),
                record.trigger,
                record.model,
                record.memory_update_count as i64,
                record.skill_update_count as i64,
                record.skipped as i64,
                record.skip_reason,
                record.duration_ms as i64,
            ],
        )
        .map_err(|e| format!("Insert review_history: {}", e))?;

        let history_id = tx.last_insert_rowid();
        let status = ReviewStatus::from_skipped(record.skipped).as_str();

        tx.execute(
            "INSERT INTO review_status
                (session_id, status, reviewed_at, trigger, model,
                 memory_update_count, skill_update_count,
                 skip_reason, duration_ms, history_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(session_id) DO UPDATE SET
                status = excluded.status,
                reviewed_at = excluded.reviewed_at,
                trigger = excluded.trigger,
                model = excluded.model,
                memory_update_count = excluded.memory_update_count,
                skill_update_count = excluded.skill_update_count,
                skip_reason = excluded.skip_reason,
                duration_ms = excluded.duration_ms,
                history_id = excluded.history_id",
            params![
                record.session_id,
                status,
                record.reviewed_at.timestamp_millis(),
                record.trigger,
                record.model,
                record.memory_update_count as i64,
                record.skill_update_count as i64,
                record.skip_reason,
                record.duration_ms as i64,
                history_id,
            ],
        )
        .map_err(|e| format!("Upsert review_status: {}", e))?;

        tx.commit().map_err(|e| format!("Commit tx: {}", e))?;
        Ok(())
    }

    /// Returns the most recent `limit` records from the audit log (newest first).
    pub fn list(&self, limit: usize) -> Result<Vec<ReviewRecord>, String> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT session_id, reviewed_at, trigger, model,
                        memory_update_count, skill_update_count,
                        skipped, skip_reason, duration_ms
                 FROM review_history
                 ORDER BY id DESC
                 LIMIT ?1",
            )
            .map_err(|e| format!("Prepare list: {}", e))?;

        let rows = stmt
            .query_map(params![limit as i64], row_to_record)
            .map_err(|e| format!("Query list: {}", e))?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(|e| format!("Row list: {}", e))?);
        }
        Ok(records)
    }

    /// Returns the latest status record for `session_id` (O(1) primary key lookup).
    pub fn find_by_session(&self, session_id: &str) -> Result<Option<ReviewRecord>, String> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT session_id, reviewed_at, trigger, model,
                        memory_update_count, skill_update_count,
                        CASE WHEN status = 'skipped' THEN 1 ELSE 0 END AS skipped,
                        skip_reason, duration_ms
                 FROM review_status
                 WHERE session_id = ?1",
            )
            .map_err(|e| format!("Prepare find: {}", e))?;

        let result = stmt
            .query_row(params![session_id], row_to_record)
            .optional()
            .map_err(|e| format!("Query find: {}", e))?;

        Ok(result)
    }

    /// Returns all session_ids that have been processed (reviewed or skipped).
    /// Sessions absent from this set are truly "pending" (never reviewed).
    pub fn reviewed_session_ids(&self) -> Result<HashSet<String>, String> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare("SELECT session_id FROM review_status")
            .map_err(|e| format!("Prepare reviewed_ids: {}", e))?;

        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Query reviewed_ids: {}", e))?;

        let mut ids = HashSet::new();
        for row in rows {
            ids.insert(row.map_err(|e| format!("Row reviewed_ids: {}", e))?);
        }
        Ok(ids)
    }

    /// Returns a `session_id → ReviewStatus` map for all reviewed/skipped sessions.
    /// Sessions not in the map are implicitly `Pending`.
    pub fn review_status_map(&self) -> Result<HashMap<String, ReviewStatus>, String> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare("SELECT session_id, status FROM review_status")
            .map_err(|e| format!("Prepare status_map: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                let session_id: String = row.get(0)?;
                let status: String = row.get(1)?;
                Ok((session_id, status))
            })
            .map_err(|e| format!("Query status_map: {}", e))?;

        let mut map = HashMap::new();
        for row in rows {
            let (session_id, status) = row.map_err(|e| format!("Row status_map: {}", e))?;
            let st = match status.as_str() {
                "reviewed" => ReviewStatus::Reviewed,
                "skipped" => ReviewStatus::Skipped,
                _ => ReviewStatus::Reviewed,
            };
            map.insert(session_id, st);
        }
        Ok(map)
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewRecord> {
    let reviewed_at_ms: i64 = row.get(1)?;
    let reviewed_at = DateTime::from_timestamp_millis(reviewed_at_ms)
        .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());
    let skipped_int: i64 = row.get(6)?;
    Ok(ReviewRecord {
        session_id: row.get(0)?,
        reviewed_at,
        trigger: row.get(2)?,
        model: row.get(3)?,
        memory_update_count: row.get::<_, i64>(4)? as usize,
        skill_update_count: row.get::<_, i64>(5)? as usize,
        skipped: skipped_int != 0,
        skip_reason: row.get(7)?,
        duration_ms: row.get::<_, i64>(8)? as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, ReviewHistory) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("memory.db");
        let history = ReviewHistory::with_db_path(db_path);
        (dir, history)
    }

    fn make_record(session_id: &str, skipped: bool) -> ReviewRecord {
        ReviewRecord {
            session_id: session_id.to_string(),
            reviewed_at: Utc::now(),
            trigger: "manual".to_string(),
            model: "gpt-4o-mini".to_string(),
            memory_update_count: 1,
            skill_update_count: 0,
            skipped,
            skip_reason: if skipped {
                Some("too_short".to_string())
            } else {
                None
            },
            duration_ms: 100,
        }
    }

    #[test]
    fn test_append_and_list() {
        let (_dir, history) = setup();
        history.append(&make_record("s1", false)).unwrap();
        history.append(&make_record("s2", false)).unwrap();
        let records = history.list(10).unwrap();
        assert_eq!(records.len(), 2);
        // newest first
        assert_eq!(records[0].session_id, "s2");
        assert_eq!(records[1].session_id, "s1");
    }

    #[test]
    fn test_list_with_limit() {
        let (_dir, history) = setup();
        for i in 0..5 {
            history
                .append(&make_record(&format!("s{}", i), false))
                .unwrap();
        }
        let records = history.list(3).unwrap();
        assert_eq!(records.len(), 3);
        // newest first: s4, s3, s2
        assert_eq!(records[0].session_id, "s4");
    }

    #[test]
    fn test_reviewed_session_ids_includes_skipped() {
        let (_dir, history) = setup();
        history.append(&make_record("s1", false)).unwrap();
        history.append(&make_record("s2", true)).unwrap();
        history.append(&make_record("s3", false)).unwrap();
        let ids = history.reviewed_session_ids().unwrap();
        // All processed sessions should be present (reviewed + skipped),
        // so that show_pending only shows truly unprocessed sessions.
        assert!(ids.contains("s1"));
        assert!(ids.contains("s2"));
        assert!(ids.contains("s3"));
    }

    #[test]
    fn test_find_by_session() {
        let (_dir, history) = setup();
        history.append(&make_record("s1", false)).unwrap();
        assert!(history.find_by_session("s1").unwrap().is_some());
        assert!(history.find_by_session("s99").unwrap().is_none());
    }

    #[test]
    fn test_review_status_map() {
        let (_dir, history) = setup();
        history.append(&make_record("s1", false)).unwrap();
        history.append(&make_record("s2", true)).unwrap();

        let map = history.review_status_map().unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("s1"), Some(&ReviewStatus::Reviewed));
        assert_eq!(map.get("s2"), Some(&ReviewStatus::Skipped));
    }

    #[test]
    fn test_upsert_keeps_latest() {
        let (_dir, history) = setup();
        // First review: success
        history.append(&make_record("s1", false)).unwrap();
        // Second review: skipped (session became too short on re-review)
        history.append(&make_record("s1", true)).unwrap();

        // review_status should reflect latest (skipped)
        let map = history.review_status_map().unwrap();
        assert_eq!(map.get("s1"), Some(&ReviewStatus::Skipped));

        // reviewed_session_ids returns all processed sessions, so s1 is present
        let ids = history.reviewed_session_ids().unwrap();
        assert!(ids.contains("s1"));

        // find_by_session should return the skipped record
        let rec = history.find_by_session("s1").unwrap().unwrap();
        assert!(rec.skipped);

        // audit log keeps both
        let all = history.list(usize::MAX).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_empty_db_returns_empty() {
        let (_dir, history) = setup();
        assert!(history.list(10).unwrap().is_empty());
        assert!(history.review_status_map().unwrap().is_empty());
        assert!(history.reviewed_session_ids().unwrap().is_empty());
        assert!(history.find_by_session("any").unwrap().is_none());
    }

    #[test]
    fn test_migrate_from_jsonl() {
        let (dir, history) = setup();
        let loom_home = dir.path();
        let review_dir = loom_home.join("data").join("review");
        std::fs::create_dir_all(&review_dir).unwrap();
        let jsonl_path = review_dir.join("history.jsonl");

        let r1 = make_record("s1", false);
        let r2 = make_record("s2", true);
        let r3 = make_record("s3", false);
        let jsonl_content = format!(
            "{}\n{}\n{}\n",
            serde_json::to_string(&r1).unwrap(),
            serde_json::to_string(&r2).unwrap(),
            serde_json::to_string(&r3).unwrap(),
        );
        std::fs::write(&jsonl_path, &jsonl_content).unwrap();

        // Opening triggers migration (table is empty + jsonl exists)
        let map = history.review_status_map().unwrap();
        assert_eq!(map.len(), 3);
        assert_eq!(map.get("s1"), Some(&ReviewStatus::Reviewed));
        assert_eq!(map.get("s2"), Some(&ReviewStatus::Skipped));
        assert_eq!(map.get("s3"), Some(&ReviewStatus::Reviewed));

        // JSONL renamed to .bak
        assert!(!jsonl_path.exists());
        assert!(jsonl_path.with_extension("jsonl.bak").exists());
    }

    #[test]
    fn test_migration_not_repeated() {
        let (dir, history) = setup();
        let loom_home = dir.path();
        let review_dir = loom_home.join("data").join("review");
        std::fs::create_dir_all(&review_dir).unwrap();
        let jsonl_path = review_dir.join("history.jsonl");

        let r1 = make_record("s1", false);
        std::fs::write(&jsonl_path, format!("{}\n", serde_json::to_string(&r1).unwrap())).unwrap();

        // First open migrates
        history.list(10).unwrap();
        assert_eq!(history.review_status_map().unwrap().len(), 1);

        // Recreate a stale jsonl to simulate "leftover" — migration should NOT re-run
        // because .bak already exists (migration marker).
        std::fs::write(&jsonl_path, format!("{}\n", serde_json::to_string(&make_record("s2", false)).unwrap())).unwrap();
        history.list(10).unwrap();
        // Still only s1, not s2
        assert_eq!(history.review_status_map().unwrap().len(), 1);
    }
}
