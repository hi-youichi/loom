//! Cross-process ownership for a running Loom server.

use fs4::fs_std::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

const DEFAULT_PID_FILE: &str = "loom-server.pid";

/// The server's default PID file: `{loom_home}/loom-server.pid`.
pub fn default_path() -> PathBuf {
    config::home::loom_home().join(DEFAULT_PID_FILE)
}

pub fn resolve_path(path: Option<&Path>) -> PathBuf {
    path.map(Path::to_path_buf).unwrap_or_else(default_path)
}

/// Holds the server ownership lock for the lifetime of the server process.
pub struct PidFileGuard {
    pid_path: PathBuf,
    lock_file: File,
    pid: u32,
}

impl PidFileGuard {
    /// Acquire ownership and publish the current process ID.
    pub fn acquire(path: impl Into<PathBuf>) -> io::Result<Self> {
        let pid_path = path.into();
        if let Some(parent) = pid_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let lock_path = lock_path(&pid_path);
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;
        lock_file
            .try_lock_exclusive()
            .map_err(|error| io::Error::new(ErrorKind::AlreadyExists, error))?;

        let pid = std::process::id();
        let temp_path = pid_path.with_extension(format!("pid.{pid}.tmp"));
        fs::write(&temp_path, format!("{pid}\n"))?;
        if let Err(error) = fs::rename(&temp_path, &pid_path) {
            let _ = fs::remove_file(&temp_path);
            let _ = lock_file.unlock();
            return Err(error);
        }

        Ok(Self {
            pid_path,
            lock_file,
            pid,
        })
    }

    pub fn path(&self) -> &Path {
        &self.pid_path
    }
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let owns_pid_file = fs::read_to_string(&self.pid_path)
            .map(|contents| contents.trim() == self.pid.to_string())
            .unwrap_or(false);
        if owns_pid_file {
            let _ = fs::remove_file(&self.pid_path);
        }
        let _ = self.lock_file.unlock();
    }
}

fn lock_path(pid_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.lock", pid_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn only_one_guard_can_own_a_pid_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("server.pid");
        let first = PidFileGuard::acquire(&path).unwrap();
        assert!(PidFileGuard::acquire(&path).is_err());
        drop(first);
        assert!(PidFileGuard::acquire(&path).is_ok());
    }
}
