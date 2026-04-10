//! Loom home directory: `$LOOM_HOME` or `~/.loom`.
//!
//! All user-level data (config, state, data) lives under a single directory.

use std::path::PathBuf;

/// Subdirectory under [`loom_home`] for per-session data: `{loom_home}/thread/{session_id}/`.
pub const THREAD_DIR: &str = "thread";

/// Returns `{loom_home}/thread/{session_id}/` (does not create directories).
pub fn thread_session_dir(session_id: &str) -> PathBuf {
    loom_home().join(THREAD_DIR).join(session_id)
}

/// `{loom_home}/acp/` — ACP server state and default log directory (does not create).
pub fn acp_data_dir() -> PathBuf {
    loom_home().join("acp")
}

/// Default ACP log file path when `--log-file` is unset (`{loom_home}/acp/loom-acp.log`).
pub fn default_acp_log_file() -> PathBuf {
    acp_data_dir().join("loom-acp.log")
}

/// Returns the Loom home directory.
///
/// Resolution: `$LOOM_HOME` env var if set, otherwise `~/.loom`.
/// Falls back to `.` if `HOME` is also unset (should not happen on real systems).
///
/// Use `LOOM_HOME` to relocate all Loom data (useful for tests and custom setups,
/// similar to `CARGO_HOME` / `RUSTUP_HOME`).
pub fn loom_home() -> PathBuf {
    if let Ok(h) = std::env::var("LOOM_HOME") {
        return PathBuf::from(h);
    }
    home_dir().join(".loom")
}

fn home_dir() -> PathBuf {
    #[cfg(unix)]
    {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    }
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    }
}

#[cfg(test)]
pub(crate) static CONFIG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(unix)]
    fn loom_home_respects_env() {
        let _lock = CONFIG_TEST_LOCK.lock().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        let test_path = "/tmp/test-loom";
        std::env::set_var("LOOM_HOME", test_path);
        assert_eq!(loom_home(), PathBuf::from(test_path));
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    #[cfg(windows)]
    fn loom_home_respects_env() {
        let _lock = CONFIG_TEST_LOCK.lock().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        let test_path = r"C:\tmp\test-loom";
        std::env::set_var("LOOM_HOME", test_path);
        assert_eq!(loom_home(), PathBuf::from(test_path));
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn thread_session_dir_under_loom_home() {
        let _lock = CONFIG_TEST_LOCK.lock().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        let test_path = "/tmp/test-loom-thread";
        std::env::set_var("LOOM_HOME", test_path);
        let expected = PathBuf::from(test_path).join("thread").join("sess-a");
        assert_eq!(super::thread_session_dir("sess-a"), expected);
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    #[cfg(windows)]
    fn thread_session_dir_under_loom_home() {
        let _lock = CONFIG_TEST_LOCK.lock().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        let test_path = r"C:\tmp\test-loom-thread";
        std::env::set_var("LOOM_HOME", test_path);
        let expected = PathBuf::from(test_path).join("thread").join("sess-a");
        assert_eq!(super::thread_session_dir("sess-a"), expected);
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn loom_home_defaults_to_dot_loom() {
        let _lock = CONFIG_TEST_LOCK.lock().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::remove_var("LOOM_HOME");
        let h = loom_home();
        assert!(h.ends_with(".loom"));
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => {}
        }
    }

    #[test]
    #[cfg(unix)]
    fn default_acp_log_file_under_acp_dir() {
        let _lock = CONFIG_TEST_LOCK.lock().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", "/tmp/loom-acp-path");
        assert_eq!(
            default_acp_log_file(),
            PathBuf::from("/tmp/loom-acp-path/acp/loom-acp.log")
        );
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    #[cfg(windows)]
    fn default_acp_log_file_under_acp_dir() {
        let _lock = CONFIG_TEST_LOCK.lock().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", r"C:\tmp\loom-acp-path");
        assert_eq!(
            default_acp_log_file(),
            PathBuf::from(r"C:\tmp\loom-acp-path\acp\loom-acp.log")
        );
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }
}
