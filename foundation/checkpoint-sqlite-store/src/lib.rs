//! SQLite-backed Checkpointer and Store implementations for anureo.
//!
//! This crate provides persistent, file-backed implementations of the
//! checkpointing and store traits defined in [`checkpoint`]:
//!
//! | Type             | Trait          | Persistence | Use case                |
//! |------------------|----------------|-------------|-------------------------|
//! | [`SqliteSaver`]  | `Checkpointer` | SQLite file | Single-node, production |
//! | [`SqliteStore`]  | `Store`        | SQLite file | Long-term key-value     |
//!
//! It also hosts the user-message history store ([`user_message`]) and shared
//! SQLite helpers ([`sqlite_util`]).

pub mod repair;
mod sqlite_saver;
mod sqlite_store;
pub mod sqlite_util;
pub mod user_message;

pub use repair::{is_malformed_db_error, repair_state_db_schema};
pub use sqlite_saver::SqliteSaver;
pub use sqlite_store::SqliteStore;
pub use sqlite_util::{
    clear_last_init_error, execute_write, format_session_db_unavailable, get_last_init_error,
    set_last_init_error, EXECUTE_WRITE_BUSY_RETRIES,
};
pub use user_message::{
    NoOpUserMessageStore, SqliteUserMessageStore, UserMessageStore, UserMessageStoreError,
};

/// Returns the default SQLite memory database path (`~/.anureo/memory.db`).
///
/// Creates the parent directory if missing. Falls back to `memory.db`
/// (cwd-relative) if the home directory is unavailable.
pub fn default_memory_db_path() -> std::path::PathBuf {
    sqlite_util::default_memory_db_path()
}

/// Global mutex for tests that modify environment variables.
pub fn env_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}
