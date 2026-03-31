//! Shared SQLite helpers (e.g. open with WAL for concurrent read/write).

use std::path::{Path, PathBuf};

const MEMORY_DB_FILENAME: &str = "memory.db";

/// Returns the default memory DB path (`~/.loom/memory.db`).
/// Creates the parent directory if missing. Falls back to `memory.db` (cwd-relative) if home is unavailable.
pub(crate) fn default_memory_db_path() -> PathBuf {
    let path = env_config::home::loom_home().join(MEMORY_DB_FILENAME);
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!("Failed to create memory db directory: {}", e);
        }
    }
    path
}

/// Builds a detailed error message for open failures: path, resolved absolute path, and cause.
fn open_error_message(path: &Path, e: &impl std::fmt::Display) -> String {
    let path_display = path.display();
    let resolved = if path.is_absolute() {
        path_display.to_string()
    } else {
        std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join(path).display().to_string())
            .unwrap_or_else(|| path_display.to_string())
    };
    format!("path='{}' resolved='{}': {}", path_display, resolved, e)
}

/// Opens a SQLite database and enables WAL mode for better concurrent read/write.
/// On failure, the error message includes the path, its resolution (cwd-relative), and the underlying cause.
pub(crate) fn open_sqlite_with_wal(path: &Path) -> Result<rusqlite::Connection, String> {
    let conn = rusqlite::Connection::open(path).map_err(|e| open_error_message(path, &e))?;
    // PRAGMA journal_mode returns a row; use execute_batch to avoid "Execute returned results".
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| open_error_message(path, &e))?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_memory_db_path_uses_loom_home() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());
        let path = default_memory_db_path();
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
        assert_eq!(path, dir.path().join("memory.db"));
    }

    #[test]
    fn default_memory_db_path_creates_parent_dir() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("sub").join("dir");
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", &nested);
        let path = default_memory_db_path();
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
        assert!(nested.exists());
        assert_eq!(path, nested.join("memory.db"));
    }

    #[test]
    fn open_error_message_absolute_path() {
        let msg = open_error_message(Path::new("/abs/path/db.sqlite"), &"some error");
        assert!(msg.contains("/abs/path/db.sqlite"));
        assert!(msg.contains("some error"));
        assert!(msg.contains("resolved="));
    }

    #[test]
    fn open_error_message_relative_path() {
        let msg = open_error_message(Path::new("relative/db.sqlite"), &"oops");
        assert!(msg.contains("relative/db.sqlite"));
        assert!(msg.contains("oops"));
    }

    #[test]
    fn open_sqlite_with_wal_creates_and_opens() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = open_sqlite_with_wal(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY);")
            .unwrap();
        assert!(db_path.exists());
    }

    #[test]
    fn open_sqlite_with_wal_invalid_path_returns_error() {
        let result = open_sqlite_with_wal(Path::new("/nonexistent/dir/db.sqlite"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("/nonexistent/dir/db.sqlite"));
    }

    #[test]
    fn memory_db_filename_constant() {
        assert_eq!(MEMORY_DB_FILENAME, "memory.db");
    }
}
