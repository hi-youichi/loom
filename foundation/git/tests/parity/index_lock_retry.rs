//! Batch 5: index.lock contention — facade-level backoff retry (100/200/400ms)
//! for write ops, verified on the default (git2) and cli backends.
//!
//! `backend_kind()` caches on first call, so each test pins the backend via
//! env before touching the facade (nextest runs each test in its own process).

use std::path::Path;
use std::time::Duration;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

fn repo_with_lock(tag: &str, permanent: bool) -> (crate::common::FixtureRepo, std::path::PathBuf) {
    let repo = crate::common::FixtureRepo::new(tag);
    repo.commit_file("a.txt", "one\n", "first");
    std::fs::write(repo.dir.join("a.txt"), "one\ntwo\n").unwrap();
    let git_dir = repo
        .git(&["rev-parse", "--absolute-git-dir"])
        .trim()
        .to_string();
    let lock_path = Path::new(&git_dir).join("index.lock");
    std::fs::write(&lock_path, if permanent { "permanent" } else { "stale" }).unwrap();
    (repo, lock_path)
}

fn run_recover(backend: &str) {
    std::env::set_var("LOOM_GIT_BACKEND", backend);
    let r = rt();
    let (repo, lock_path) = repo_with_lock("lockrec", false);

    let repo_path = repo.path().to_path_buf();
    let res = r.block_on(async {
        let lock = lock_path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(220)).await;
            std::fs::remove_file(&lock).ok();
        });
        loom_git::facade::stage_file(&repo_path, "a.txt").await
    });
    res.unwrap_or_else(|e| panic!("stage ({backend}) should recover from lock: {e}"));

    let st = r.block_on(loom_git::facade::status(&repo_path)).unwrap();
    assert!(
        st.files.iter().any(|f| f.path == "a.txt"),
        "({backend}) a.txt must be staged after recovery"
    );
}

fn run_exhaust(backend: &str) {
    std::env::set_var("LOOM_GIT_BACKEND", backend);
    let r = rt();
    let repo = crate::common::FixtureRepo::new("lockexh");
    repo.commit_file("a.txt", "one\n", "first");
    std::fs::write(repo.dir.join("a.txt"), "one\ntwo\n").unwrap();

    let repo_path = repo.path().to_path_buf();
    r.block_on(loom_git::facade::stage_file(&repo_path, "a.txt"))
        .unwrap_or_else(|e| panic!("({backend}) pre-stage failed: {e}"));

    // lock after staging so the commit's staged-check passes and the
    // index write itself hits the lock
    let git_dir = repo
        .git(&["rev-parse", "--absolute-git-dir"])
        .trim()
        .to_string();
    let lock_path = Path::new(&git_dir).join("index.lock");
    std::fs::write(&lock_path, "permanent").unwrap();

    let res = r.block_on(loom_git::facade::stage_file(&repo_path, "a.txt"));
    match res {
        Err(e) => assert!(
            matches!(e.kind(), loom_git::GitErrorKind::Locked),
            "({backend}) expected Locked after retry exhaustion, got {e:?}"
        ),
        Ok(_) => panic!("({backend}) stage must not succeed while index.lock is held"),
    }
}

#[test]
fn index_lock_backoff_recovers_git2() {
    run_recover("git2");
}

#[test]
fn index_lock_backoff_recovers_cli() {
    run_recover("cli");
}

#[test]
fn index_lock_backoff_exhausts_to_locked_git2() {
    run_exhaust("git2");
}

#[test]
fn index_lock_backoff_exhausts_to_locked_cli() {
    run_exhaust("cli");
}
