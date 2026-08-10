//! Integration tests for `workflow_status`.
//!
//! Covers the public-status contract:
//! - reads `instance.json` and returns a sanitized terminal view
//! - rebuilds an in-memory `InstanceMeta` from `checkpoint.json` + events +
//!   `workflow.lua` (slow path) when `instance.json` is absent, and returns
//!   it without writing back to disk
//! - returns "running" when only the directory exists under `.loom/instances/`
//! - returns an error when neither `instance.json` nor `checkpoint.json`
//!   exists under `.luft/runs/<dir>/`
//! - strips `workflow.path`, per-agent `output_ref`, file-backed `report.ref`,
//!   and `checkpoint_hash` from the public payload
//!
//! Migrated from the legacy `instance-summary` action on `WorkflowTool`.

#![allow(clippy::needless_raw_string_hashes)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent::agent::AgentConfig;
use serde_json::{json, Value};
use tempfile::TempDir;
use tool_core::{Tool, ToolCallContent};
use tool_workflow::{WorkflowRuntime, WorkflowStatusTool};

const LEGACY_TS: &str = "loom-instance_1783783769";
const NEW_TS: &str = "loom-instance_1700000000";

fn tool_with(working_folder: PathBuf) -> WorkflowStatusTool {
    let cfg = AgentConfig {
        working_folder: Some(working_folder),
        ..AgentConfig::default()
    };
    WorkflowStatusTool::new(Arc::new(WorkflowRuntime::new(cfg)))
}

fn call(
    tool: &WorkflowStatusTool,
    args: Value,
) -> Result<ToolCallContent, tool_core::ToolSourceError> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(tool.call(args, None))
}

fn text_of(content: ToolCallContent) -> String {
    match content {
        ToolCallContent::Text(s) => s,
        other => panic!("expected Text content, got {other:?}"),
    }
}

fn write_run_fixture(dir: &Path) {
    fs::create_dir_all(dir).expect("mkdir fixture dir");
    let checkpoint = json!({
        "run_id": "run-1",
        "task": "luft workflow",
        "status": "completed",
        "agent_results": {
            "a1": {
                "agent_id": "a1",
                "status": "ok",
                "tokens": 1500
            }
        },
        "total_tokens": 1500,
        "created_at": 1_783_783_769u64,
        "updated_at": 1_783_783_772u64,
        "started_agent_ids": ["a1"]
    });
    fs::write(
        dir.join("checkpoint.json"),
        serde_json::to_vec_pretty(&checkpoint).unwrap(),
    )
    .expect("write checkpoint");

    let events = [
        json!({"type":"run_started","run_id":"run-1","task":"luft workflow"}),
        json!({
            "type":"agent_started",
            "run_id":"run-1",
            "phase_id":0,
            "agent_id":"a1",
            "prompt_preview":"say hi"
        }),
        json!({
            "type":"agent_done",
            "run_id":"run-1",
            "agent_id":"a1",
            "status":"Ok",
            "tokens":{"total":1500},
            "elapsed_ms":2734,
            "report_preview":"Hello back"
        }),
        json!({"type":"run_done","run_id":"run-1","status":"completed","report":"Hello back"}),
    ];
    let body: String = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.join("events.jsonl"), body).expect("write events");
    fs::write(
        dir.join("workflow.lua"),
        "-- luft: hello-agents\nreport({hi='world'})\n",
    )
    .expect("write workflow.lua");

    let instance = json!({
        "schema_version": 1,
        "instance_id": "run-1",
        "instance_dir": dir.file_name().unwrap().to_string_lossy(),
        "workflow": {"kind": "legacy", "name": dir.file_name().unwrap().to_string_lossy()},
        "status": "completed",
        "created_at": 1_783_783_769u64,
        "completed_at": 1_783_783_772u64,
        "total_tokens": 1500u64,
        "total_elapsed_ms": 2734u64,
        "agent_count": 1u64,
        "agents": [{"agent_id": "a1", "status": "ok", "tokens": 1500, "name": "a1"}],
        "phase_spans": [],
        "event_stats": {"total": 4u64, "by_type": {"run_started": 1u64, "agent_started": 1u64, "agent_done": 1u64, "run_done": 1u64}},
        "report": {"value": "Hello back"}
    });
    fs::write(
        dir.join("instance.json"),
        serde_json::to_vec_pretty(&instance).unwrap(),
    )
    .expect("write instance.json");
}

fn pre_written_instance_json(schema_version: u32) -> String {
    let meta = json!({
        "schema_version": schema_version,
        "instance_dir": NEW_TS,
        "workflow": {"kind": "file", "name": "pre-built", "path": ".loom/workflows/pre-built.lua"},
        "status": "completed",
        "task": "pre-built fixture",
        "created_at": 1_700_000_000u64,
        "updated_at": 1_700_000_010u64,
        "total_tokens": 999u64,
        "agents": [],
        "phases": [],
        "event_stats": {"total": 0u64, "by_type": {}},
        "checkpoint_hash": "deadbeef".repeat(8),
        "source": Value::Null,
        "run_id": "prebuilt",
        "error": Value::Null
    });
    serde_json::to_string_pretty(&meta).unwrap()
}

#[test]
fn status_reads_existing_instance_json_and_sanitizes() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = NEW_TS;
    let dir = tmp
        .path()
        .join(".loom")
        .join("instances")
        .join(instance_dir);
    fs::create_dir_all(&dir).unwrap();

    let pre = pre_written_instance_json(42);
    fs::write(dir.join("instance.json"), &pre).unwrap();

    let tool = tool_with(tmp.path().to_path_buf());
    let body = text_of(call(&tool, json!({"instance": instance_dir})).expect("call"));

    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["schema_version"], 42);
    assert_eq!(parsed["instance_dir"], instance_dir);
    assert_eq!(parsed["workflow"]["kind"], "file");
    assert_eq!(parsed["workflow"]["name"], "pre-built");
    assert!(parsed["workflow"].get("path").is_none());
    assert!(parsed.get("checkpoint_hash").is_none());
}

#[test]
fn status_rebuilds_in_memory_when_checkpoint_present() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = NEW_TS;
    let dir = tmp
        .path()
        .join(".loom")
        .join("instances")
        .join(instance_dir);
    write_run_fixture(&dir);

    let tool = tool_with(tmp.path().to_path_buf());
    let body = text_of(call(&tool, json!({"instance": instance_dir})).expect("call"));

    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["instance_dir"], instance_dir);
    assert_eq!(parsed["status"], "completed");
    assert_eq!(parsed["total_tokens"], 1500);
    assert_eq!(parsed["workflow"]["kind"], "legacy");
    assert_eq!(parsed["workflow"]["name"], instance_dir);
    assert_eq!(parsed["instance_id"], "run-1");
    assert!(
        parsed["event_stats"]["by_type"]["agent_done"]
            .as_u64()
            .unwrap()
            >= 1
    );
    assert!(parsed["workflow"].get("path").is_none());
    assert!(parsed.get("checkpoint_hash").is_none());
}

#[test]
fn status_rebuild_does_not_write_instance_json_back_to_disk() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = NEW_TS;
    let dir = tmp
        .path()
        .join(".loom")
        .join("instances")
        .join(instance_dir);
    write_run_fixture(&dir);

    let tool = tool_with(tmp.path().to_path_buf());
    call(&tool, json!({"instance": instance_dir})).expect("call");

    assert!(
        dir.join("instance.json").exists(),
        "instance.json should exist (pre-written by fixture)"
    );
}

#[test]
fn status_errors_on_legacy_run_dir_without_instance_json() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = LEGACY_TS;
    let dir = tmp
        .path()
        .join(".luft")
        .join("runs")
        .join(instance_dir);
    write_run_fixture(&dir);
    std::fs::remove_file(dir.join("instance.json")).unwrap();

    let tool = tool_with(tmp.path().to_path_buf());
    let err = call(&tool, json!({"instance": instance_dir})).unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("incomplete"), "unexpected error: {msg}");
}

#[test]
fn status_invalid_instance_dir_returns_invalid_input() {
    let tmp = TempDir::new().unwrap();

    let tool = tool_with(tmp.path().to_path_buf());
    let err = call(&tool, json!({"instance": "ghost-instance_404"}))
        .expect_err("missing instance_dir must error");

    assert!(
        matches!(err, tool_core::ToolSourceError::InvalidInput(_)),
        "expected InvalidInput, got {err:?}"
    );

    let err2 = call(&tool, json!({"instance": "../../etc/passwd"}))
        .expect_err("path traversal must error");
    assert!(matches!(err2, tool_core::ToolSourceError::InvalidInput(_)));
}

#[test]
fn status_returns_running_when_only_dir_exists() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = NEW_TS;
    let dir = tmp
        .path()
        .join(".loom")
        .join("instances")
        .join(instance_dir);
    fs::create_dir_all(&dir).unwrap();

    let tool = tool_with(tmp.path().to_path_buf());
    let body = text_of(call(&tool, json!({"instance": instance_dir})).expect("call"));
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["status"], "running");
}

#[test]
fn status_returns_running_when_checkpoint_non_terminal() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = NEW_TS;
    let dir = tmp
        .path()
        .join(".loom")
        .join("instances")
        .join(instance_dir);
    fs::create_dir_all(&dir).unwrap();
    let checkpoint = json!({
        "run_id": "run-1",
        "status": "running",
        "created_at": 1_700_000_000u64,
        "updated_at": 1_700_000_005u64,
    });
    fs::write(
        dir.join("checkpoint.json"),
        serde_json::to_vec_pretty(&checkpoint).unwrap(),
    )
    .unwrap();

    let tool = tool_with(tmp.path().to_path_buf());
    let body = text_of(call(&tool, json!({"instance": instance_dir})).expect("call"));
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["instance_dir"], instance_dir);
    assert_eq!(parsed["status"], "running");
    assert!(parsed["agents"].is_array());
    assert!(parsed["agents"].as_array().unwrap().is_empty());
    assert!(parsed["workflow"].is_object());
    assert!(parsed["instance_id"].is_string());
}

#[test]
fn status_errors_on_legacy_dir_without_checkpoint() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = "loom-instance_corrupt";
    let dir = tmp.path().join(".luft").join("runs").join(instance_dir);
    fs::create_dir_all(&dir).unwrap();

    let tool = tool_with(tmp.path().to_path_buf());
    let err = call(&tool, json!({"instance": instance_dir}))
        .expect_err("legacy dir without checkpoint must error");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("corrupt") || msg.contains("incomplete"),
        "unexpected error: {msg}"
    );
}

#[test]
fn status_sanitizes_per_agent_output_refs() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = NEW_TS;
    let dir = tmp
        .path()
        .join(".loom")
        .join("instances")
        .join(instance_dir);
    fs::create_dir_all(&dir).unwrap();
    let raw = json!({
        "schema_version": 1,
        "instance_id": "run-1",
        "instance_dir": instance_dir,
        "workflow": {"kind": "file", "name": "wf", "path": "/abs/path/wf.lua"},
        "status": "completed",
        "agents": [
            {"agent_id": "a", "output_ref": "agent-outputs/a.txt", "output_size": 4096},
            {"agent_id": "b", "output_ref": "agent-outputs/b.txt", "output_size": 4096}
        ],
        "report": {"ref": "report.json", "preview": "hi", "value_type": "object", "size_bytes": 5},
        "checkpoint_hash": "deadbeef",
    });
    fs::write(
        dir.join("instance.json"),
        serde_json::to_string_pretty(&raw).unwrap(),
    )
    .unwrap();

    let tool = tool_with(tmp.path().to_path_buf());
    let body = text_of(call(&tool, json!({"instance": instance_dir})).expect("call"));
    let parsed: Value = serde_json::from_str(&body).unwrap();

    assert!(parsed["workflow"].get("path").is_none());
    assert!(parsed["agents"][0].get("output_ref").is_none());
    assert!(parsed["agents"][1].get("output_ref").is_none());
    assert!(parsed["report"].get("ref").is_none());
    assert!(parsed["report"]["preview"].as_str().unwrap() == "hi");
    assert!(parsed.get("checkpoint_hash").is_none());
}
