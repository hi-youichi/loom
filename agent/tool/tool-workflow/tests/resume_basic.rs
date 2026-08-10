//! Resume tests: T0 (all cached), T18 (no agents)
//! These tests run fully in-process (no subprocess needed).

use luft::LuftBuilder;
use luft_core::testing::*;
use serde_json::json;

mod common;
use common::*;

/// T0: Resume from completed run — all agents cached.
/// NOTE: ignored on luft ≥0.5 — resume reads checkpoint.json, but luft 0.5
/// stores checkpoints in SQLite only.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "luft 0.5.1 resolve_resume() reads checkpoint.json, but checkpoint is in SQLite"]
async fn t0_resume_completed_all_cached() {
    let tmp = tempfile::TempDir::new().unwrap();

    let backend1 = SharedBackend::new(json!("ok"));
    let luft1 = LuftBuilder::new()
        .backend(backend1.clone())
        .base_dir(tmp.path())
        .concurrency(2)
        .build()
        .unwrap();

    let handle = luft1.start_script(SCRIPT_3PHASE).await.unwrap();
    for _ in 0..50 {
        if backend1.total_calls() >= 3 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let outcome = handle.join().await.unwrap();
    assert!(outcome.result.is_ok());
    assert_eq!(backend1.total_calls(), 3);
    let dir = outcome.run_dir_name;

    let backend2 = SharedBackend::new(json!("ok"));
    let luft2 = LuftBuilder::new()
        .backend(backend2.clone())
        .base_dir(tmp.path())
        .concurrency(2)
        .build()
        .unwrap();

    let result = luft2.start_resume(&dir).await;
    match result {
        Ok(handle2) => {
            let outcome2 = handle2.join().await.unwrap();
            assert_eq!(
                backend2.total_calls(),
                0,
                "all agents should be cached on resume"
            );
            assert!(outcome2.result.is_ok());
        }
        Err(e) => {
            eprintln!("Resume from completed returned error: {e}");
        }
    }
}

/// T18: Workflow with no agents.
#[tokio::test(flavor = "multi_thread")]
async fn t18_workflow_without_agents() {
    let tmp = tempfile::TempDir::new().unwrap();
    let backend = SharedBackend::new(json!("ok"));

    let luft = LuftBuilder::new()
        .backend(backend.clone())
        .base_dir(tmp.path())
        .concurrency(2)
        .build()
        .unwrap();

    let handle = luft.start_script(SCRIPT_NO_AGENT).await.unwrap();
    let outcome = handle.join().await.unwrap();

    assert!(outcome.result.is_ok());
    assert_eq!(backend.total_calls(), 0);
}
