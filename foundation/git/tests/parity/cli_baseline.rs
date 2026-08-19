//! Parity harness: construct fixture repos, run backend methods, assert JSON equality.
//! B1 will diff CliBackend vs Git2Backend; P0 asserts CliBackend invariants
//! against the legacy extension contract.

use std::path::{Path, PathBuf};
use std::process::Command;

use loom_git::cli::{run_process_sync, CliBackend};
use loom_git::types::GitFileStatus;
use loom_git::GitBackend;

pub struct FixtureRepo {
    pub dir: PathBuf,
}

fn git(dir: &Path, args: &[&str]) -> String {
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
        git(&dir, &["init", "-b", "main"]);
        git(&dir, &["config", "user.name", "Test User"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        Self { dir }
    }

    pub fn commit_file(&self, name: &str, content: &str, message: &str) {
        let path = self.dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&path, content).unwrap();
        git(&self.dir, &["add", name]);
        git(&self.dir, &["commit", "-m", message]);
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

fn has_git() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn cli_status_clean_repo() {
    if !has_git() {
        return;
    }
    let repo = FixtureRepo::new("clean");
    repo.commit_file("a.txt", "hello\n", "initial");
    let backend = CliBackend::new();
    let status = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(backend.status(repo.path()))
        .expect("status");
    assert_eq!(status.branch, "main");
    assert!(status.files.is_empty());
    assert!(status.in_progress.is_none());
}

#[test]
fn cli_status_dirty_repo() {
    if !has_git() {
        return;
    }
    let repo = FixtureRepo::new("dirty");
    repo.commit_file("a.txt", "hello\n", "initial");
    std::fs::write(repo.dir.join("a.txt"), "changed\n").unwrap();
    std::fs::write(repo.dir.join("b.txt"), "new\n").unwrap();
    let backend = CliBackend::new();
    let status = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(backend.status(repo.path()))
        .expect("status");
    assert_eq!(status.files.len(), 2);
    assert!(status
        .files
        .iter()
        .any(|f| f.path == "a.txt" && matches!(f.working_status, GitFileStatus::Modified)));
    assert!(status
        .files
        .iter()
        .any(|f| f.path == "b.txt" && matches!(f.index_status, GitFileStatus::Untracked)));
}

#[test]
fn cli_log_and_branches() {
    if !has_git() {
        return;
    }
    let repo = FixtureRepo::new("log");
    repo.commit_file("a.txt", "1\n", "first");
    repo.commit_file("a.txt", "2\n", "second");
    let backend = CliBackend::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let log = rt
        .block_on(backend.log(
            repo.path(),
            &loom_git::backend::LogQuery {
                limit: 10,
                skip: 0,
                branch: None,
                file_path: None,
            },
        ))
        .expect("log");
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].message, "second");
    assert_eq!(log[0].parents.len(), 1);

    let branches = rt
        .block_on(backend.branches(repo.path(), false))
        .expect("branches");
    assert_eq!(branches.len(), 1);
    assert!(branches[0].is_current);
    assert_eq!(branches[0].name, "main");
}

#[test]
fn cli_diff_dirty_file() {
    if !has_git() {
        return;
    }
    let repo = FixtureRepo::new("diff");
    repo.commit_file("a.txt", "line1\nline2\n", "initial");
    std::fs::write(repo.dir.join("a.txt"), "line1\nline2\nline3\n").unwrap();
    let backend = CliBackend::new();
    let summary = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(backend.diff(repo.path(), false, None, 3))
        .expect("diff");
    assert_eq!(summary.hunks.len(), 1);
    assert_eq!(summary.stat.insertions, 1);
    assert_eq!(summary.stat.files_changed, 1);
    let hunk = &summary.hunks[0];
    assert_eq!(hunk.new_start, 1);
    assert_eq!(hunk.new_lines, 3);
}

#[test]
fn cli_not_a_repo_yields_not_found() {
    if !has_git() {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "loom-git-parity-notrepo-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let backend = CliBackend::new();
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(backend.status(&dir));
    match result {
        Err(e) => {
            assert!(matches!(e.kind(), loom_git::GitErrorKind::NotFound));
        }
        Ok(_) => panic!("expected NotFound"),
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cli_sync_helpers() {
    if !has_git() {
        return;
    }
    let repo = FixtureRepo::new("synchelp");
    repo.commit_file("a.txt", "x\n", "c");

    let ok = loom_git::cli::run_process_sync(repo.path(), &["rev-parse", "--short", "HEAD"])
        .expect("spawn");
    assert!(ok.status.success());
    let bad = loom_git::cli::run_process_sync(repo.path(), &["log", "--definitely-not-a-flag"])
        .expect("spawn");
    assert!(
        !bad.status.success(),
        "non-zero exit must surface as Ok(Output)"
    );

    let out = loom_git::cli::run_string_sync(repo.path(), &["rev-parse", "--short", "HEAD"])
        .expect("run_string_sync");
    assert!(!out.is_empty());

    let root = loom_git::cli::resolve_repo_root(repo.path()).expect("repo root");
    assert!(
        root.ends_with(repo.dir.file_name().unwrap()),
        "resolve_repo_root must return the worktree root, got {} for {}",
        root.display(),
        repo.dir.display()
    );

    let r = tokio::runtime::Runtime::new().unwrap();
    let o = r
        .block_on(loom_git::cli::run_process(
            Some(repo.path()),
            &["rev-parse", "--short", "HEAD"],
        ))
        .expect("async spawn");
    assert!(o.status.success());

    // error classification on the sync string path
    let err = loom_git::cli::run_string_sync(
        &repo.dir.join("missing-dir"),
        &["rev-parse", "--short", "HEAD"],
    );
    assert!(err.is_err(), "non-existent cwd must fail to spawn");
}
