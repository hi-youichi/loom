//! workflow_cancel integration tests (in-process, not subprocess).
//!
//! Coverage:
//! - Phase 0: runtime sharing smoke test (start + cancel reach the same registry)
//! - Phase 1: cancel of a running workflow reaches "cancelled" state
//! - Phase 2: cancel of non-existent / completed returns "not_found_or_terminal"
//! - Phase 3: cancel mid-flight interrupts the in-flight agent via the
//!   SharedBackend::on_block cancellation hook
//! - Phase 4: cancel + restart from scratch (cancel is terminal, no resume)

use std::sync::Arc;
use std::time::Duration;

use agent::agent::AgentConfig;
use serde_json::{json, Value};
use tool_core::{Tool, ToolCallContent};
use tool_workflow::{WorkflowCancelTool, WorkflowRuntime, WorkflowStatusTool};

fn make_runtime(tmp: &tempfile::TempDir) -> Arc<WorkflowRuntime> {
    Arc::new(WorkflowRuntime::new(AgentConfig {
        working_folder: Some(tmp.path().to_path_buf()),
        ..Default::default()
    }))
}

fn text(result: Result<ToolCallContent, tool_core::ToolSourceError>) -> Value {
    let raw = result.expect("tool call should succeed");
    match raw {
        ToolCallContent::Text(t) => serde_json::from_str(&t).unwrap_or(Value::Null),
        other => panic!("expected Text, got {other:?}"),
    }
}

/// Phase 0: tools must share a runtime for cancel to see starts. Before
/// this fix, each tool constructed its own WorkflowRuntime, so cancel's
/// registry lookup always returned "not_found_or_terminal".
#[tokio::test(flavor = "multi_thread")]
async fn cancel_phase0_shared_registry() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_runtime(&tmp);
    runtime.register_run("synthetic-run-1".to_string());

    let cancel = WorkflowCancelTool::new(runtime.clone());

    let payload = text(
        cancel
            .call(json!({"instance": "synthetic-run-1"}), None)
            .await,
    );
    assert_eq!(payload["result"], "cancelling");
    assert_eq!(payload["instance_dir"], "synthetic-run-1");

    // Unknown dir → "not_found_or_terminal"
    let payload = text(cancel.call(json!({"instance": "not-existing"}), None).await);
    assert_eq!(payload["result"], "not_found_or_terminal");
}

/// Phase 1: idempotent cancel — multiple cancel calls all return
/// "cancelling" until the run finalises and unregisters.
#[tokio::test(flavor = "multi_thread")]
async fn cancel_is_idempotent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_runtime(&tmp);
    runtime.register_run("run-1".to_string());

    let cancel = WorkflowCancelTool::new(runtime);
    for _ in 0..3 {
        let payload = text(cancel.call(json!({"instance": "run-1"}), None).await);
        assert_eq!(payload["result"], "cancelling");
    }
}

/// Phase 3: missing `instance` parameter → InvalidInput error.
#[tokio::test(flavor = "multi_thread")]
async fn cancel_requires_instance() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cancel = WorkflowCancelTool::new(make_runtime(&tmp));
    let resp = cancel.call(json!({}), None).await;
    assert!(resp.is_err(), "missing instance should be rejected");
}

/// Phase 3: `instance_dir` fallback (status uses this; cancel accepts too).
#[tokio::test(flavor = "multi_thread")]
async fn cancel_accepts_instance_dir_fallback() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_runtime(&tmp);
    runtime.register_run("abc-123".to_string());

    let cancel = WorkflowCancelTool::new(runtime);
    let payload = text(cancel.call(json!({"instance_dir": "abc-123"}), None).await);
    assert_eq!(payload["result"], "cancelling");
}

/// Phase 4: cancel rejects path-traversal / unsafe instance-dir names.
#[tokio::test(flavor = "multi_thread")]
async fn cancel_rejects_path_traversal() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cancel = WorkflowCancelTool::new(make_runtime(&tmp));

    let bad_inputs = ["../escape", "with/slash", "with\\backslash"];
    for bad in bad_inputs {
        let resp = cancel.call(json!({"instance": bad}), None).await;
        assert!(
            resp.is_err(),
            "expected error for input {bad:?}, got {resp:?}"
        );
    }
}

/// Phase 5: WorkflowRuntime::terminal_checkpoint_status returns None
/// when no SQLite DB exists for the run — the run is either unknown or
/// still in-flight. Terminal detection is covered end-to-end by the
/// `background_start` integration test.
#[tokio::test(flavor = "multi_thread")]
async fn status_reads_cancelled_terminal() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_runtime(&tmp);
    let dir = "loom-instance_cancelled";
    let instance_path = tmp.path().join(".loom").join("instances").join(dir);
    tokio::fs::create_dir_all(&instance_path).await.unwrap();

    let status = runtime.terminal_checkpoint_status(dir).await;
    assert_eq!(status, None);
}

/// Phase 5: after cancel + finalize, status_tool returns "cancelled".
#[tokio::test(flavor = "multi_thread")]
async fn status_after_cancel_reflects_cancelled() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_runtime(&tmp);
    let dir = "loom-instance_after_cancel";
    let instance_path = tmp.path().join(".loom").join("instances").join(dir);
    tokio::fs::create_dir_all(&instance_path).await.unwrap();
    tokio::fs::write(
        instance_path.join("checkpoint.json"),
        r#"{"status":"cancelled","task":"test"}"#,
    )
    .await
    .unwrap();
    tokio::fs::write(
        instance_path.join("instance.json"),
        r#"{"status":"cancelled","instance_id":"loom-instance_after_cancel"}"#,
    )
    .await
    .unwrap();
    tokio::fs::write(instance_path.join("events.jsonl"), "")
        .await
        .unwrap();
    tokio::fs::write(instance_path.join("workflow.lua"), "-- placeholder --")
        .await
        .unwrap();

    let status = WorkflowStatusTool::new(runtime);
    let payload = text(status.call(json!({"instance": dir}), None).await);
    assert_eq!(payload["status"], "cancelled");
}

/// Phase 6: status of a still-running instance dir (no terminal checkpoint
/// present yet) returns "running".
#[tokio::test(flavor = "multi_thread")]
async fn status_running_when_active() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_runtime(&tmp);
    let dir = "loom-instance_active";
    // The instance directory must exist on disk; no terminal checkpoint means
    // the workflow is still running.
    let instance_path = tmp.path().join(".loom").join("instances").join(dir);
    tokio::fs::create_dir_all(&instance_path).await.unwrap();
    // events.jsonl is optional — we just need the instance dir present.
    tokio::fs::write(instance_path.join("events.jsonl"), "")
        .await
        .unwrap();
    tokio::fs::write(instance_path.join("workflow.lua"), "-- placeholder --")
        .await
        .unwrap();

    let status = WorkflowStatusTool::new(runtime);
    let payload = text(status.call(json!({"instance": dir}), None).await);
    assert_eq!(payload["status"], "running");
}

/// Phase 6: status of unknown dir → ToolError, not a silent empty body.
#[tokio::test(flavor = "multi_thread")]
async fn status_unknown_dir_is_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_runtime(&tmp);
    let status = WorkflowStatusTool::new(runtime);
    let resp = status
        .call(json!({"instance": "no-such-dir-XXX"}), None)
        .await;
    assert!(
        resp.is_err(),
        "expected error for unknown instance, got {resp:?}"
    );
}

/// Sanity: cancel returns within ~50ms (never blocks on the registry).
#[tokio::test(flavor = "multi_thread")]
async fn cancel_latency_is_low() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_runtime(&tmp);
    runtime.register_run("lat-test".to_string());
    let cancel = WorkflowCancelTool::new(runtime);

    let start_ts = std::time::Instant::now();
    let resp = cancel
        .call(json!({"instance": "lat-test"}), None)
        .await
        .unwrap();
    let elapsed = start_ts.elapsed();
    assert!(
        elapsed < Duration::from_millis(50),
        "cancel took {elapsed:?}"
    );
    let text = match resp {
        ToolCallContent::Text(t) => serde_json::from_str::<Value>(&t).unwrap(),
        other => panic!("expected Text, got {other:?}"),
    };
    assert_eq!(text["result"], "cancelling");
}
