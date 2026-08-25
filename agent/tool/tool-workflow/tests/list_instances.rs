//! Integration tests for `workflow_list`.
//!
//! Each test seeds `.luft/runs/` or `.anureo/instances/` directly on disk
//! under a temporary `working_folder`, then invokes the tool through its
//! public `call()` interface and parses the JSON returned in
//! `ToolCallContent::Text`. The schema and error contract are pinned by
//! `agent/tool/tool-workflow/src/tool.rs::spec()` and exercised here so
//! regressions in pagination, filtering, or tag behaviour trip CI fast.
//!
//! Migrated from the legacy `list-instances` action on `WorkflowTool`.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use agent::agent::AgentConfig;
use serde_json::{json, Value};
use tempfile::tempdir;
use tool_core::{Tool, ToolCallContent};
use tool_workflow::{WorkflowListTool, WorkflowRuntime};

fn build_tool(working_folder: PathBuf) -> WorkflowListTool {
    let cfg = AgentConfig {
        working_folder: Some(working_folder),
        ..Default::default()
    };
    WorkflowListTool::new(Arc::new(WorkflowRuntime::new(cfg)))
}

async fn call_list(tool: &WorkflowListTool, args: Value) -> Result<ToolCallContent, String> {
    match tool.call(args, None).await {
        Ok(c) => Ok(c),
        Err(e) => Err(format!("{e:?}")),
    }
}

fn parse_response(content: ToolCallContent) -> Value {
    let ToolCallContent::Text(s) = content else {
        panic!("expected text content, got {content:?}");
    };
    serde_json::from_str(&s).expect("response must be valid JSON")
}

fn write_checkpoint(dir: &std::path::Path, status: &str, created_at: u64) {
    fs::create_dir_all(dir).unwrap();
    let dir_name = dir.file_name().unwrap().to_string_lossy().to_string();
    let payload = json!({
        "schema_version": 1,
        "instance_id": format!("run-{}", dir_name),
        "instance_dir": dir_name,
        "workflow": {"kind": "file", "name": dir_name},
        "status": status,
        "created_at": created_at,
        "completed_at": created_at + 1,
        "total_tokens": 100u64,
        "agent_count": 1u64,
    });
    let bytes = serde_json::to_vec(&payload).unwrap();
    fs::write(dir.join("instance.json"), bytes).unwrap();
}

fn write_instance(
    dir: &std::path::Path,
    instance_id: &str,
    status: &str,
    created_at: u64,
    workflow_kind: &str,
    workflow_name: &str,
) {
    fs::create_dir_all(dir).unwrap();
    let payload = json!({
        "schema_version": 1,
        "instance_id": instance_id,
        "instance_dir": dir.file_name().unwrap().to_string_lossy().to_string(),
        "workflow": {
            "kind": workflow_kind,
            "name": workflow_name,
            "path": "/some/where/.anureo/workflows/example.lua",
        },
        "status": status,
        "created_at": created_at,
        "completed_at": created_at + 1,
        "total_tokens": 100u64,
        "agent_count": 1u64,
    });
    let bytes = serde_json::to_vec(&payload).unwrap();
    fs::write(dir.join("instance.json"), bytes).unwrap();
}

fn seed_legacy(working_folder: &std::path::Path, count: usize, status: &str) {
    let runs = working_folder.join(".luft").join("runs");
    for i in 0..count {
        let ca: u64 = 1_700_000_000 + i as u64;
        write_checkpoint(&runs.join(format!("run_{i:02}")), status, ca);
    }
}

fn instance_dirs(value: &Value) -> Vec<String> {
    value["instances"]
        .as_array()
        .expect("instances must be an array")
        .iter()
        .map(|e| {
            e["instance_dir"]
                .as_str()
                .expect("instance_dir must be a string")
                .to_string()
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn list_default_limit_is_20() {
    let tmp = tempdir().unwrap();
    seed_legacy(tmp.path(), 25, "completed");
    let tool = build_tool(tmp.path().to_path_buf());

    let resp = call_list(&tool, json!({})).await.unwrap();
    let v = parse_response(resp);

    assert_eq!(v["count"], 20, "default limit should be 20; got {v}");
    assert_eq!(
        v["has_more"],
        serde_json::Value::Bool(true),
        "25 entries with limit 20 must report has_more=true"
    );
    assert!(
        v["next_cursor"].as_str().is_some(),
        "has_more=true implies next_cursor must be a string"
    );
    assert_eq!(instance_dirs(&v).len(), 20);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_limit_clamped_to_100() {
    let tmp = tempdir().unwrap();
    let tool = build_tool(tmp.path().to_path_buf());

    let err = call_list(&tool, json!({"limit": 200})).await.unwrap_err();
    let msg = err.to_lowercase();
    assert!(
        msg.contains("limit") && msg.contains("100"),
        "limit=200 must produce a clear error mentioning 'limit' and '100'; got {err:?}"
    );

    let err = call_list(&tool, json!({"limit": 0})).await.unwrap_err();
    assert!(
        err.to_lowercase().contains("limit"),
        "limit=0 must be rejected with an InvalidInput error; got {err:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn list_cursor_returns_next_page() {
    let tmp = tempdir().unwrap();
    seed_legacy(tmp.path(), 30, "completed");
    let tool = build_tool(tmp.path().to_path_buf());

    let page1 = call_list(&tool, json!({"limit": 10})).await.unwrap();
    let v1 = parse_response(page1);
    assert_eq!(v1["count"], 10);
    assert_eq!(v1["has_more"], serde_json::Value::Bool(true));
    let cursor1 = v1["next_cursor"].as_str().unwrap().to_string();
    let page1_dirs = instance_dirs(&v1);
    let last_p1 = page1_dirs.last().unwrap().clone();
    assert_eq!(cursor1, last_p1);

    let page2 = call_list(&tool, json!({"limit": 10, "cursor": cursor1}))
        .await
        .unwrap();
    let v2 = parse_response(page2);
    assert_eq!(v2["count"], 10);
    assert_eq!(
        v2["has_more"],
        serde_json::Value::Bool(true),
        "page 2 of 3 (30 entries / page size 10) must still have more"
    );

    let page2_dirs = instance_dirs(&v2);
    for d in &page2_dirs {
        assert!(
            !page1_dirs.contains(d),
            "page 2 must not repeat page 1 entries ({d})"
        );
    }
    assert_eq!(
        page2_dirs.first().unwrap(),
        "run_19",
        "page 2 first dir should be exactly the entry after last_p1 in desc order"
    );
    assert_eq!(
        page2_dirs.last().unwrap(),
        "run_10",
        "page 2 last should be run_10"
    );

    let cursor2 = v2["next_cursor"].as_str().unwrap().to_string();
    let page3 = call_list(&tool, json!({"limit": 10, "cursor": cursor2}))
        .await
        .unwrap();
    let v3 = parse_response(page3);
    assert_eq!(v3["count"], 10);
    assert_eq!(
        v3["has_more"],
        serde_json::Value::Bool(false),
        "page 3 (final) must report has_more=false"
    );
    assert!(v3["next_cursor"].is_null());
    let page3_dirs = instance_dirs(&v3);
    assert_eq!(page3_dirs.first().unwrap(), "run_09");
    assert_eq!(page3_dirs.last().unwrap(), "run_00");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_cursor_null_on_last_page() {
    let tmp = tempdir().unwrap();
    seed_legacy(tmp.path(), 5, "completed");
    let tool = build_tool(tmp.path().to_path_buf());

    let resp = call_list(&tool, json!({"limit": 20})).await.unwrap();
    let v = parse_response(resp);
    assert_eq!(v["count"], 5);
    assert_eq!(v["has_more"], serde_json::Value::Bool(false));
    assert!(
        v["next_cursor"].is_null(),
        "last page must return next_cursor=null (got {})",
        v["next_cursor"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn list_status_filter_failed_excludes_completed() {
    let tmp = tempdir().unwrap();
    let runs = tmp.path().join(".luft").join("runs");
    let statuses = ["completed", "failed", "cancelled", "completed", "failed"];
    for (i, s) in statuses.iter().enumerate() {
        write_checkpoint(
            &runs.join(format!("run_{i:02}")),
            s,
            1_700_000_000 + i as u64,
        );
    }
    let tool = build_tool(tmp.path().to_path_buf());

    let resp = call_list(&tool, json!({"status_filter": "failed"}))
        .await
        .unwrap();
    let v = parse_response(resp);
    let instances = v["instances"].as_array().unwrap();
    assert_eq!(instances.len(), 2, "expected 2 failed entries, got {v}");
    for inst in instances {
        assert_eq!(inst["status"], "failed");
    }

    let resp = call_list(&tool, json!({"status_filter": "FAILED"}))
        .await
        .unwrap();
    let v = parse_response(resp);
    assert_eq!(v["instances"].as_array().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_status_filter_invalid_returns_invalid_input_error() {
    let tmp = tempdir().unwrap();
    let tool = build_tool(tmp.path().to_path_buf());

    let err = call_list(&tool, json!({"status_filter": "reject_this"}))
        .await
        .unwrap_err();
    assert!(
        err.contains("InvalidInput"),
        "expected InvalidInput variant, got {err}"
    );
    assert!(
        err.to_lowercase().contains("status_filter"),
        "error should mention the offending parameter; got {err}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn list_legacy_luft_runs_without_internal_source_field() {
    let tmp = tempdir().unwrap();
    let legacy = tmp.path().join(".luft").join("runs");
    write_checkpoint(&legacy.join("legacy_run_01"), "completed", 1_700_000_000);

    let tool = build_tool(tmp.path().to_path_buf());
    let resp = call_list(&tool, json!({})).await.unwrap();
    let v = parse_response(resp);
    let instances = v["instances"].as_array().unwrap();
    assert_eq!(instances.len(), 1, "expected 1 entry; got {v}");
    assert!(instances[0].get("source").is_none());
    assert_eq!(instances[0]["workflow"]["kind"], "file");
    assert_eq!(instances[0]["workflow"]["name"], "legacy_run_01");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_current_instances_without_internal_source_field() {
    let tmp = tempdir().unwrap();
    let current = tmp.path().join(".anureo").join("instances");
    write_instance(
        &current.join("anureo-instance_42"),
        "inst-42",
        "completed",
        1_700_000_000,
        "file",
        "refactor",
    );

    let tool = build_tool(tmp.path().to_path_buf());
    let resp = call_list(&tool, json!({})).await.unwrap();
    let v = parse_response(resp);
    let instances = v["instances"].as_array().unwrap();
    assert_eq!(instances.len(), 1, "expected 1 entry; got {v}");
    let entry = &instances[0];
    assert!(entry.get("source").is_none());
    assert_eq!(entry["instance_id"], "inst-42");
    assert_eq!(entry["workflow"]["kind"], "file");
    assert_eq!(entry["workflow"]["name"], "refactor");
    assert!(
        entry["workflow"].get("path").is_none(),
        "workflow_list entry must omit 'path' field (got {entry:?})"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn list_invalid_cursor_returns_error() {
    let tmp = tempdir().unwrap();
    let tool = build_tool(tmp.path().to_path_buf());

    let err = call_list(&tool, json!({"cursor": "no_such_dir"}))
        .await
        .unwrap_err();
    assert!(
        err.contains("ToolError"),
        "expected ToolError variant; got {err}"
    );
    assert!(
        err.contains("cursor not found"),
        "error must explain that the cursor was not found; got {err}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn list_includes_all_instance_json_entries() {
    let tmp = tempdir().unwrap();
    let runs = tmp.path().join(".luft").join("runs");
    write_checkpoint(&runs.join("run_done"), "completed", 1_700_000_000);
    write_checkpoint(&runs.join("run_alive"), "running", 1_700_000_001);
    write_checkpoint(&runs.join("run_pending"), "RUNNING", 1_700_000_002);
    write_checkpoint(&runs.join("run_weird"), "queued", 1_700_000_003);

    let tool = build_tool(tmp.path().to_path_buf());
    let resp = call_list(&tool, json!({})).await.unwrap();
    let v = parse_response(resp);

    let dirs = instance_dirs(&v);
    assert_eq!(
        dirs.len(),
        4,
        "all instance.json entries should appear; got {v}"
    );
    assert_eq!(v["count"], 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_empty_when_directory_missing() {
    let tmp = tempdir().unwrap();
    let tool = build_tool(tmp.path().to_path_buf());

    let resp = call_list(&tool, json!({})).await.unwrap();
    let v = parse_response(resp);
    assert_eq!(v["count"], 0);
    assert!(
        v["next_cursor"].is_null(),
        "empty list must return next_cursor=null (got {})",
        v["next_cursor"]
    );
    assert_eq!(v["has_more"], serde_json::Value::Bool(false));
    assert!(instance_dirs(&v).is_empty());
}
