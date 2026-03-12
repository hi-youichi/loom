//! Shared SQLite helpers (e.g. open with WAL for concurrent read/write).

use std::path::{Path, PathBuf};

/// XDG app name for default memory db path (e.g. `$XDG_DATA_HOME/loom/memory.db`).
const XDG_APP_NAME: &str = "loom";
const MEMORY_DB_FILENAME: &str = "memory.db";

/// Returns the default memory DB path under XDG data home (e.g. `$XDG_DATA_HOME/loom/memory.db`).
/// Creates the parent directory if missing. Falls back to `memory.db` (cwd-relative) if XDG base dirs are unavailable.
pub(crate) fn default_memory_db_path() -> PathBuf {
    let path = match cross_xdg::BaseDirs::new() {
        Ok(base) => base.data_home().join(XDG_APP_NAME).join(MEMORY_DB_FILENAME),
        Err(_) => PathBuf::from(MEMORY_DB_FILENAME),
    };
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
    format!(
        "path='{}' resolved='{}': {}",
        path_display, resolved, e
    )
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
