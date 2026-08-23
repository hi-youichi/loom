//! Process-global Loom home directory: `--home` override, else `~/.loom`.
//!
//! All user-level data (config, state, data) lives under a single directory.
//! The override is set once at process start by the CLI `--home` flag
//! (see `apps/cli/src/main.rs`); child processes must receive `--home`
//! explicitly because the value is no longer propagated via environment.
//!
//! Hosted in its own crate so `loom-config` and `model-spec-core` can share
//! the single override without a dependency cycle.

use std::path::PathBuf;
use std::sync::RwLock;

static OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Set (or clear, when `None`) the process-global home override.
pub fn set_override(path: Option<PathBuf>) {
    let mut guard = OVERRIDE.write().unwrap_or_else(|e| e.into_inner());
    *guard = path;
}

/// The currently active override, if any.
pub fn override_path() -> Option<PathBuf> {
    OVERRIDE.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Returns the Loom home directory: the `--home` override if set, else
/// `~/.loom` (Windows: `%USERPROFILE%\.loom`). Falls back to `.` when the
/// user home cannot be determined (should not happen on real systems).
pub fn loom_home() -> PathBuf {
    if let Some(h) = override_path() {
        return h;
    }
    user_home().join(".loom")
}

fn user_home() -> PathBuf {
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
    fn override_wins_until_cleared() {
        let dir = tempfile::tempdir().unwrap();
        set_override(Some(dir.path().to_path_buf()));
        assert_eq!(loom_home(), dir.path());
        set_override(None);
        assert_eq!(loom_home(), user_home().join(".loom"));
    }
}
