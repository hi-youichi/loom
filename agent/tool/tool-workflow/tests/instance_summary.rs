//! Integration tests for the `instance-summary` action on `WorkflowTool`.
//!
//! Covers the T-05 contract:
//! - returns the curated `InstanceMeta` (caches as `instance.json`)
//! - rebuilds on the fly when `instance.json` is absent
//! - persists `instance.json` after rebuilding
//! - excludes the raw `events` array (only `event_stats`)
//! - falls back to legacy `.luft/runs/<dir>/` artefacts during the migration
//! - rejects unknown `instance_dir` with `InvalidInput`
//!
//! Same tempdir-style fixture as the T-01 / T-03 smoke tests.
#![allow(clippy::needless_raw_string_hashes)]

use std::fs;
use std::path::{Path, PathBuf};

use agent::agent::AgentConfig;
use serde_json::{json, Value};
use tempfile::TempDir;
use tool_core::{Tool, ToolCallContent};
use tool_workflow::WorkflowTool;

const LEGACY_TS: &str = "loom-instance_1783783769";
const NEW_TS: &str = "loom-instance_1700000000";

/// Construct a `WorkflowTool` rooted at the given working folder.
fn tool_with(working_folder: PathBuf) -> WorkflowTool {
    let cfg = AgentConfig {
        working_folder: Some(working_folder),
        ..AgentConfig::default()
    };
    WorkflowTool::new(cfg)
}

/// Invoke the tool's async `call` from a sync `#[test]`.
fn call(tool: &WorkflowTool, args: Value) -> Result<ToolCallContent, tool_core::ToolSourceError> {
    let rt = tokio::runtime::Builder::new_current_thread()
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

/// Write a hand-rolled but realistic checkpoint + events + workflow.lua
/// fixture so `build_instance_meta` produces a meaningful, structured
/// `InstanceMeta`. Mirrors the shape used by `luft` itself.
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
    fs::write(dir.join("checkpoint.json"), serde_json::to_vec_pretty(&checkpoint).unwrap())
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
    fs::write(dir.join("workflow.lua"), "-- luft: hello-agents\nreport({hi='world'})\n")
        .expect("write workflow.lua");
}

/// Build a hand-written `instance.json` payload for the
/// "reads existing instance.json" case.
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
fn summary_reads_existing_instance_json() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = NEW_TS;
    let dir = tmp.path().join(".loom").join("instances").join(instance_dir);
    fs::create_dir_all(&dir).unwrap();

    let pre = pre_written_instance_json(42);
    fs::write(dir.join("instance.json"), &pre).unwrap();

    let tool = tool_with(tmp.path().to_path_buf());
    let body = text_of(
        call(
            &tool,
            json!({"action": "instance-summary", "instance_dir": instance_dir}),
        )
        .expect("call"),
    );

    // Round-trips the pre-written payload verbatim, pretty-printed. The custom
    // schema_version (42) is our marker that no rebuild happened.
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["schema_version"], 42);
    assert_eq!(parsed["instance_dir"], instance_dir);
    assert_eq!(parsed["workflow"]["kind"], "file");
    assert_eq!(parsed["workflow"]["name"], "pre-built");
}

#[test]
fn summary_builds_on_the_fly_when_missing() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = NEW_TS;
    let dir = tmp.path().join(".loom").join("instances").join(instance_dir);
    write_run_fixture(&dir);
    // Sanity: no instance.json pre-existed.
    assert!(!dir.join("instance.json").exists());

    let tool = tool_with(tmp.path().to_path_buf());
    let body = text_of(
        call(
            &tool,
            json!({"action": "instance-summary", "instance_dir": instance_dir}),
        )
        .expect("call"),
    );

    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["instance_dir"], instance_dir);
    assert_eq!(parsed["status"], "completed");
    assert_eq!(parsed["total_tokens"], 1500);
    assert_eq!(parsed["workflow"]["kind"], "legacy");
    assert_eq!(parsed["workflow"]["name"], instance_dir);
    // Recovers the run id embedded in the checkpoint.
    assert_eq!(parsed["instance_id"], "run-1");
    // One agent_done event rolls into event_stats.
    assert!(parsed["event_stats"]["by_type"]["agent_done"].as_u64().unwrap() >= 1);
}

#[test]
fn summary_persists_instance_json_after_build() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = NEW_TS;
    let dir = tmp.path().join(".loom").join("instances").join(instance_dir);
    write_run_fixture(&dir);

    let tool = tool_with(tmp.path().to_path_buf());
    call(
        &tool,
        json!({"action": "instance-summary", "instance_dir": instance_dir}),
    )
    .expect("call");

    // instance.json must now exist *and* be re-readable JSON with the
    // expected fields, so the next `instance-summary` call would hit the
    // fast path.
    let path = dir.join("instance.json");
    assert!(path.is_file(), "instance.json should be persisted at {path:?}");
    let raw = fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed["instance_dir"], instance_dir);
    assert_eq!(parsed["workflow"]["kind"], "legacy");

    // A subsequent call should return the same payload (idempotent cache).
    let second = text_of(
        call(
            &tool,
            json!({"action": "instance-summary", "instance_dir": instance_dir}),
        )
        .expect("call"),
    );
    let second_parsed: Value = serde_json::from_str(&second).unwrap();
    assert_eq!(second_parsed["instance_dir"], parsed["instance_dir"]);
}

#[test]
fn summary_excludes_raw_events_array() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = NEW_TS;
    let dir = tmp.path().join(".loom").join("instances").join(instance_dir);
    write_run_fixture(&dir);

    let tool = tool_with(tmp.path().to_path_buf());
    let body = text_of(
        call(
            &tool,
            json!({"action": "instance-summary", "instance_dir": instance_dir}),
        )
        .expect("call"),
    );
    let parsed: Value = serde_json::from_str(&body).unwrap();

    // Per the T-05 contract: no top-level "events" array ΓÇö callers that need
    // the raw stream should use `instance-events`.
    assert!(
        parsed.get("events").is_none(),
        "instance-summary must NOT include a top-level events array; got: {parsed}"
    );
}

#[test]
fn summary_event_stats_present() {
    let tmp = TempDir::new().unwrap();
    let instance_dir = NEW_TS;
    let dir = tmp.path().join(".loom").join("instances").join(instance_dir);
    write_run_fixture(&dir);

    let tool = tool_with(tmp.path().to_path_buf());
    let body = text_of(
        call(
            &tool,
            json!({"action": "instance-summary", "instance_dir": instance_dir}),
        )
        .expect("call"),
    );
    let parsed: Value = serde_json::from_str(&body).unwrap();

    // event_stats always present with total + by_type (the curated summary).
    let stats = parsed
        .get("event_stats")
        .expect("event_stats should be present");
    assert!(stats["total"].as_u64().unwrap() >= 1);
    let by_type = stats["by_type"].as_object().expect("by_type object");
    // Our fixture includes run_started, agent_started, agent_done, run_done.
    assert!(by_type.contains_key("agent_done"));
    assert!(by_type.contains_key("run_started"));
}

#[test]
fn summary_legacy_dir_builds_and_persists() {
    let tmp = TempDir::new().unwrap();
    // Pre-merge fixtures live under the historical `.luft/runs/<dir>/`
    // path. The handler should recognise that path, build the meta, and
    // write `instance.json` back into the SAME directory so subsequent
    // queries hit the fast path.
    let dir = tmp
        .path()
        .join(".luft")
        .join("runs")
        .join(LEGACY_TS);
    write_run_fixture(&dir);
    assert!(!dir.join("instance.json").exists());

    let tool = tool_with(tmp.path().to_path_buf());
    let body = text_of(
        call(
            &tool,
            json!({"action": "instance-summary", "instance_dir": LEGACY_TS}),
        )
        .expect("call"),
    );
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["instance_dir"], LEGACY_TS);
    assert_eq!(parsed["status"], "completed");
    assert_eq!(parsed["workflow"]["kind"], "legacy");

    // Persisted in the legacy dir exactly where the artefacts already live.
    let instance_path = dir.join("instance.json");
    assert!(
        instance_path.is_file(),
        "instance.json should be persisted to the legacy dir; looked at {instance_path:?}"
    );
    let raw = fs::read_to_string(&instance_path).unwrap();
    let persisted: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(persisted["instance_dir"], LEGACY_TS);
}

#[test]
fn summary_invalid_instance_dir_returns_invalid_input() {
    let tmp = TempDir::new().unwrap();

    // No dir exists under either the new or the legacy root.
    let tool = tool_with(tmp.path().to_path_buf());
    let err = call(
        &tool,
        json!({"action": "instance-summary", "instance_dir": "ghost-instance_404"}),
    )
    .expect_err("missing instance_dir must error");

    // The contract is InvalidInput (caller-correctable), not ToolError.
    assert!(
        matches!(err, tool_core::ToolSourceError::InvalidInput(_)),
        "expected InvalidInput, got {err:?}"
    );

    // Path-traversal attempts are also rejected at the input layer.
    let err2 = call(
        &tool,
        json!({"action": "instance-summary", "instance_dir": "../../etc/passwd"}),
    )
    .expect_err("path traversal must error");
    assert!(matches!(
        err2,
        tool_core::ToolSourceError::InvalidInput(_)
    ));
}
