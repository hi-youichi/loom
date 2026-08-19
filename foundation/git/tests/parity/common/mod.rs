use std::path::{Path, PathBuf};

use loom_git::cli::run_process_sync;

pub struct FixtureRepo {
    pub dir: PathBuf,
}

fn run_git_at(dir: &Path, args: &[&str]) -> String {
    let output = run_process_sync(dir, args).expect("git command in fixture");
    if !output.status.success() {
        panic!(
            "fixture git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8_lossy(&output.stdout).to_string()
}

impl FixtureRepo {
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "loom-git-parity-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("tempdir");
        run_git_at(&dir, &["init", "-b", "main"]);
        run_git_at(&dir, &["config", "user.name", "Test User"]);
        run_git_at(&dir, &["config", "user.email", "test@example.com"]);
        Self { dir }
    }

    pub fn commit_file(&self, name: &str, content: &str, message: &str) {
        let path = self.dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&path, content).unwrap();
        self.git(&["add", name]);
        self.git(&["commit", "-m", message]);
    }

    /// Run a git command inside the fixture, expect success, return stdout.
    pub fn git(&self, args: &[&str]) -> String {
        run_git_at(&self.dir, args)
    }

    /// Run a git command, tolerating non-zero exit (e.g. merge conflicts).
    pub fn git_raw(&self, args: &[&str]) -> (bool, String, String) {
        let output = run_process_sync(&self.dir, args).expect("git command spawn");
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )
    }

    /// Run a git command at an arbitrary path (e.g. creating a bare origin).
    pub fn git_at(&self, path: &Path, args: &[&str]) -> String {
        if !path.exists() && (args.contains(&"--bare") || args.first() == Some(&"clone")) {
            std::fs::create_dir_all(path).expect("create dir");
        }
        run_git_at(path, args)
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for FixtureRepo {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

#[allow(dead_code)]
pub fn has_git() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
