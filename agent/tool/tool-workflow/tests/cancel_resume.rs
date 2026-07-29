//! workflow_cancel + workflow_start(resume_from_id) integration tests.
//!
//! Two tracks:
//! 1. **Pure tool-layer cancel** — exercise `WorkflowCancelTool` against a
//!    runtime registry without involving the full luft lifecycle.
//! 2. **Engine-level resume** — exercise `luft.start_resume` against a
//!    completed run (no in-process cancel; the subprocess crash tests
//!    already cover that path).

use std::sync::Arc;

use luft::LuftBuilder;
use luft_core::testing::SharedBackend;
use serde_json::json;

const SCRIPT_LONG: &str = r#"
function main()
  phase("collect")
  agent({name = "a1", prompt = "prompt-1"})
  phase("analyze")
  agent({name = "a2", prompt = "prompt-2"})
  phase("report")
  agent({name = "a3", prompt = "prompt-3"})
  report({ok = true})
end
"#;

// ── Track 1: tool-layer cancel (no crash, no resume) ────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn cancel_tool_registers_and_cancels_through_registry() {
    use tool_core::Tool;
    use tool_workflow::{WorkflowCancelTool, WorkflowRuntime};

    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = agent::agent::AgentConfig {
        working_folder: Some(tmp.path().to_path_buf()),
        ..Default::default()
    };
    let runtime = Arc::new(WorkflowRuntime::new(cfg));

    // Register a synthetic run (simulates what start_workflow does).
    runtime.register_run("loom-instance_42".to_string());

    let cancel = WorkflowCancelTool::new(runtime.clone());
    let resp = cancel
        .call(json!({"instance": "loom-instance_42"}), None)
        .await
        .unwrap();
    let text = match resp {
        tool_core::ToolCallContent::Text(t) => t,
        other => panic!("expected Text, got {other:?}"),
    };
    let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(payload["result"], "cancelling");
    assert_eq!(payload["instance_dir"], "loom-instance_42");

    // Same instance id is still cancellable (idempotent registry hit).
    let resp2 = cancel
        .call(json!({"instance": "loom-instance_42"}), None)
        .await
        .unwrap();
    let text2 = match resp2 {
        tool_core::ToolCallContent::Text(t) => t,
        other => panic!("expected Text, got {other:?}"),
    };
    let payload2: serde_json::Value = serde_json::from_str(&text2).unwrap();
    assert_eq!(payload2["result"], "cancelling");
}

#[tokio::test(flavor = "current_thread")]
async fn cancel_tool_unknown_dir_returns_not_found_terminal() {
    use tool_core::Tool;
    use tool_workflow::{WorkflowCancelTool, WorkflowRuntime};

    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = agent::agent::AgentConfig {
        working_folder: Some(tmp.path().to_path_buf()),
        ..Default::default()
    };
    let runtime = Arc::new(WorkflowRuntime::new(cfg));

    let cancel = WorkflowCancelTool::new(runtime);
    let resp = cancel
        .call(json!({"instance": "no-such-instance"}), None)
        .await
        .unwrap();
    let text = match resp {
        tool_core::ToolCallContent::Text(t) => t,
        other => panic!("expected Text, got {other:?}"),
    };
    let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(payload["result"], "not_found_or_terminal");
}

#[tokio::test(flavor = "current_thread")]
async fn cancel_tool_rejects_missing_instance() {
    use tool_core::Tool;
    use tool_workflow::{WorkflowCancelTool, WorkflowRuntime};

    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = agent::agent::AgentConfig {
        working_folder: Some(tmp.path().to_path_buf()),
        ..Default::default()
    };
    let cancel = WorkflowCancelTool::new(Arc::new(WorkflowRuntime::new(cfg)));
    let resp = cancel.call(json!({}), None).await;
    assert!(resp.is_err(), "missing instance parameter should error");
}

#[tokio::test(flavor = "current_thread")]
async fn cancel_tool_rejects_path_traversal() {
    use tool_core::Tool;
    use tool_workflow::{WorkflowCancelTool, WorkflowRuntime};

    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = agent::agent::AgentConfig {
        working_folder: Some(tmp.path().to_path_buf()),
        ..Default::default()
    };
    let cancel = WorkflowCancelTool::new(Arc::new(WorkflowRuntime::new(cfg)));

    let bad = ["../escape", "with/slash", "with\\backslash"];
    for input in bad {
        let resp = cancel.call(json!({"instance": input}), None).await;
        assert!(
            resp.is_err(),
            "expected error for input {input:?}, got {resp:?}"
        );
    }
}

// ── Track 2: engine-level resume (no cancel involved) ───────────────────────

/// Resume a *completed* workflow: luft's journal cache must mean zero
/// re-dispatch — the canonical "resume is free after completion" case.
#[tokio::test(flavor = "current_thread")]
async fn resume_after_completion_dispatches_nothing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let backend = SharedBackend::new(json!("ok"));
    let luft = LuftBuilder::new()
        .backend(backend.clone())
        .base_dir(tmp.path())
        .concurrency(2)
        .build()
        .unwrap();

    let handle = luft.start_script(SCRIPT_LONG).await.unwrap();
    let outcome = handle.join().await.unwrap();
    assert!(outcome.result.is_ok());
    let dir = outcome.run_dir_name;

    // Resume with a cloned backend; all state is Arc-shared.
    let backend2 = backend.clone();
    let luft2 = LuftBuilder::new()
        .backend(backend2.clone())
        .base_dir(tmp.path())
        .concurrency(2)
        .build()
        .unwrap();
    let handle2 = luft2.start_resume(&dir).await.unwrap();
    let outcome2 = handle2.join().await.unwrap();
    assert!(outcome2.result.is_ok());

    // All three agents were cached → zero new dispatches on resume.
    assert_eq!(
        backend.total_calls(),
        3,
        "only the original 3 calls should have happened; resume dispatches nothing"
    );
}

/// Resume with a non-existent dir returns Err cleanly without hanging.
#[tokio::test(flavor = "current_thread")]
async fn resume_nonexistent_dir_errors_quickly() {
    let tmp = tempfile::TempDir::new().unwrap();
    let backend = SharedBackend::new(json!("ok"));
    let luft = LuftBuilder::new()
        .backend(backend.clone())
        .base_dir(tmp.path())
        .concurrency(2)
        .build()
        .unwrap();

    let start = luft.start_resume("does_not_exist_xxx");
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), start).await;
    match result {
        Ok(Ok(_)) => panic!("resume of nonexistent dir should error"),
        Ok(Err(_)) => {} // expected
        Err(_) => panic!("resume should error within 5s, did not"),
    }
}
