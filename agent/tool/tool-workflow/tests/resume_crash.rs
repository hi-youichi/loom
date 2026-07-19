//! Crash-and-resume integration tests using subprocess.
//!
//! Pattern:
//! 1. Spawn `resume_child` binary with crash_after=N
//! 2. Wait for child to exit(1)
//! 3. Resume in-process with CountingBackend
//! 4. Assert agent dispatch counts

use luft::LuftBuilder;
use luft_core::testing::CountingBackend;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

fn child_bin_path() -> PathBuf {
    let exe = if cfg!(windows) { "resume_child.exe" } else { "resume_child" };
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let candidates = [
        PathBuf::from(manifest_dir).join("../../target/debug").join(exe),
        PathBuf::from(manifest_dir).join("../../../target/debug").join(exe),
    ];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    std::env::var("CARGO_BIN_EXE_resume_child")
        .map(PathBuf::from)
        .unwrap_or_else(|_| candidates[0].clone())
}

fn run_child_and_crash(base_dir: &Path, script: &str, crash_after: u64) -> String {
    let bin = child_bin_path();
    let output = Command::new(&bin)
        .arg(base_dir)
        .arg(script)
        .arg(crash_after.to_string())
        .output()
        .expect("failed to spawn resume_child");

    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("[parent] child stderr:\n{stderr}");

    assert!(
        !output.status.success(),
        "child should have crashed, but exited with: {:?}",
        output.status
    );

    stderr
        .lines()
        .find_map(|l| l.strip_prefix("[child] run_dir: ").map(String::from))
        .expect("child should have printed run_dir name")
}

/// T1: Phase 之间崩溃 — a1 完成，a2 crash
#[test]
fn t1_crash_between_phases() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = run_child_and_crash(tmp.path(), "3phase", 2);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let backend = CountingBackend::new(json!("ok"));
        let luft = LuftBuilder::new()
            .backend(backend.clone())
            .base_dir(tmp.path())
            .concurrency(2)
            .build()
            .unwrap();

        let handle = luft.start_resume(&dir).await.expect("resume");
        let outcome = handle.join().await.expect("join");

        assert_eq!(backend.dispatched_names(), vec!["a2", "a3"]);
        assert!(outcome.result.is_ok());
    });
}

/// T3: Phase 内多 agent 之间崩溃
#[test]
fn t3_crash_between_agents_in_phase() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = run_child_and_crash(tmp.path(), "multi", 2);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let backend = CountingBackend::new(json!("ok"));
        let luft = LuftBuilder::new()
            .backend(backend.clone())
            .base_dir(tmp.path())
            .concurrency(1)
            .build()
            .unwrap();

        let handle = luft.start_resume(&dir).await.expect("resume");
        let outcome = handle.join().await.expect("join");

        assert_eq!(backend.dispatched_names(), vec!["a2", "a3", "a4"]);
        assert!(outcome.result.is_ok());
    });
}

/// T8: 接近完成时崩溃
#[test]
fn t8_crash_near_completion() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = run_child_and_crash(tmp.path(), "3phase", 3);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let backend = CountingBackend::new(json!("ok"));
        let luft = LuftBuilder::new()
            .backend(backend.clone())
            .base_dir(tmp.path())
            .concurrency(1)
            .build()
            .unwrap();

        let handle = luft.start_resume(&dir).await.expect("resume");
        let outcome = handle.join().await.expect("join");

        assert_eq!(backend.dispatched_names(), vec!["a3"]);
        assert!(outcome.result.is_ok());
    });
}
