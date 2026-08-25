//! anureo home directory: `--home` override or `~/.anureo`.
//!
//! All user-level data (config, state, data) lives under a single directory.
//! The resolution state itself lives in the `anureo-home` crate so that
//! `model-spec-core` can share it without depending on `anureo-config`.

pub use anureo_home::{anureo_home, override_path, set_override};

use std::path::PathBuf;

/// Subdirectory under [`anureo_home`] for per-session data: `{anureo_home}/thread/{session_id}/`.
pub const THREAD_DIR: &str = "thread";

/// Returns `{anureo_home}/thread/{session_id}/` (does not create directories).
pub fn thread_session_dir(session_id: &str) -> PathBuf {
    anureo_home().join(THREAD_DIR).join(session_id)
}

/// `{anureo_home}/acp/` — ACP server state and default log directory (does not create).
pub fn acp_data_dir() -> PathBuf {
    anureo_home().join("acp")
}

/// `{anureo_home}/logs/` — Unified logs directory (does not create).
pub fn logs_dir() -> PathBuf {
    anureo_home().join("logs")
}

/// `{anureo_home}/logs/llm/` — LLM audit log directory.
pub fn llm_logs_dir() -> PathBuf {
    logs_dir().join("llm")
}

/// Default unified log file path: `{anureo_home}/anureo.log`.
///
/// Both CLI and ACP use this as the default log file.
pub fn default_log_file() -> PathBuf {
    anureo_home().join("anureo.log")
}

#[cfg(test)]
pub(crate) static CONFIG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_session_dir_under_home() {
        let _lock = CONFIG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        set_override(Some(dir.path().to_path_buf()));
        assert_eq!(
            thread_session_dir("abc"),
            dir.path().join("thread").join("abc")
        );
        set_override(None);
    }

    #[test]
    fn acp_data_dir_under_home() {
        let _lock = CONFIG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        set_override(Some(dir.path().to_path_buf()));
        assert_eq!(acp_data_dir(), dir.path().join("acp"));
        set_override(None);
    }

    #[test]
    fn logs_dirs_under_home() {
        let _lock = CONFIG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        set_override(Some(dir.path().to_path_buf()));
        assert_eq!(logs_dir(), dir.path().join("logs"));
        assert_eq!(llm_logs_dir(), dir.path().join("logs").join("llm"));
        set_override(None);
    }

    #[test]
    fn default_log_file_under_home() {
        let _lock = CONFIG_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        set_override(Some(dir.path().to_path_buf()));
        assert_eq!(default_log_file(), dir.path().join("anureo.log"));
        set_override(None);
    }
}
