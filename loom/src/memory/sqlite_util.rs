//! Shared SQLite helpers (e.g. open with WAL for concurrent read/write).

use std::path::Path;

/// Opens a SQLite database and enables WAL mode for better concurrent read/write.
pub(crate) fn open_sqlite_with_wal(path: &Path) -> Result<rusqlite::Connection, String> {
    let conn = rusqlite::Connection::open(path).map_err(|e| e.to_string())?;
    // PRAGMA journal_mode returns a row; use execute_batch to avoid "Execute returned results".
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| e.to_string())?;
    Ok(conn)
}
