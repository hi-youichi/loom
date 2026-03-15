//! Loom home directory: `$LOOM_HOME` or `~/.loom`.
//!
//! All user-level data (config, state, data) lives under a single directory.

use std::path::PathBuf;

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
mod tests {
    use super::*;

    #[test]
    fn loom_home_respects_env() {
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", "/tmp/test-loom");
        assert_eq!(loom_home(), PathBuf::from("/tmp/test-loom"));
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn loom_home_defaults_to_dot_loom() {
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::remove_var("LOOM_HOME");
        let h = loom_home();
        assert!(h.ends_with(".loom"));
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => {}
        }
    }
}
