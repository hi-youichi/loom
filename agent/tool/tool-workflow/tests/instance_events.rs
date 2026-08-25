//! Tests for `workflow_events`.
//!
//! These tests construct a `WorkflowEventsTool` whose `working_folder`
//! points at a tempdir, write a synthetic `events.jsonl` under
//! `<working_folder>/.anureo/instances/<instance_dir>/`, and exercise
//! `Tool::call` with various filter / pagination shapes.
//!
//! Migrated from the legacy `instance-events` action on `WorkflowTool`.

use agent::agent::AgentConfig;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use tool_core::{Tool, ToolSourceError};
use tool_workflow::{WorkflowEventsTool, WorkflowRuntime};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

fn synthetic_events_jsonl() -> String {
    let lines = [
        r#"{"type":"run_started","run_id":"r1","ts":"2026-07-01T00:00:00Z"}"#,
        r#"{"type":"agent_started","agent_id":"a1","prompt_preview":"alpha","model":null}"#,
        r#"{"type":"agent_progress","agent_id":"a1","delta":{"kind":"message","text":"hello"}}"#,
        r#"{"type":"agent_progress","agent_id":"a1","delta":{"kind":"message","text":" world"}}"#,
        r#"{"type":"agent_done","agent_id":"a1","status":"Ok","tokens":{},"elapsed_ms":120}"#,
        r#"{"type":"agent_started","agent_id":"a2","prompt_preview":"beta","model":null}"#,
        r#"{"type":"agent_progress","agent_id":"a2","delta":{"kind":"message","text":"..."}}"#,
        r#"{"type":"agent_done","agent_id":"a2","status":"Ok","tokens":{},"elapsed_ms":80}"#,
        r#"{"type":"run_progress","run_id":"r1","phase":"planning","progress":0.5}"#,
        r#"{"type":"run_progress","run_id":"r1","phase":"planning","progress":1.0}"#,
        r#"{"type":"agent_started","agent_id":"a1","prompt_preview":"gamma","model":null}"#,
        r#"{"type":"agent_done","agent_id":"a1","status":"Failed","tokens":{},"elapsed_ms":60}"#,
        r#"{"type":"run_progress","run_id":"r1","phase":"reporting","progress":0.2}"#,
        r#"{"type":"run_progress","run_id":"r1","phase":"reporting","progress":0.6}"#,
        r#"{"type":"run_progress","run_id":"r1","phase":"reporting","progress":1.0}"#,
        r#"{"type":"agent_started","agent_id":"a3","prompt_preview":"delta","model":null}"#,
        r#"{"type":"agent_progress","agent_id":"a3","delta":{"kind":"message","text":"hi"}}"#,
        r#"{"type":"agent_progress","agent_id":"a3","delta":{"kind":"message","text":"!"}}"#,
        r#"{"type":"agent_done","agent_id":"a3","status":"Ok","tokens":{},"elapsed_ms":40}"#,
        r#"{"type":"run_done","run_id":"r1","status":"Ok","total_tokens":0}"#,
    ];
    lines.join("\n") + "\n"
}

fn build_tool(dir: &Path) -> WorkflowEventsTool {
    let cfg = AgentConfig {
        working_folder: Some(dir.to_path_buf()),
        ..AgentConfig::default()
    };
    WorkflowEventsTool::new(Arc::new(WorkflowRuntime::new(cfg)))
}

fn setup_instance(dir: &Path, instance_dir: &str) -> WorkflowEventsTool {
    let inst_path: PathBuf = dir.join(".anureo").join("instances").join(instance_dir);
    fs::create_dir_all(&inst_path).expect("mkdir .anureo/instances/<dir>");
    fs::write(inst_path.join("events.jsonl"), synthetic_events_jsonl())
        .expect("write events.jsonl");
    build_tool(dir)
}

async fn call_text(tool: &WorkflowEventsTool, args: Value) -> Value {
    let content = tool
        .call(args, None)
        .await
        .expect("tool call should succeed");
    match content {
        tool_core::ToolCallContent::Text(text) => {
            serde_json::from_str(&text).expect("response should be JSON")
        }
        other => panic!("expected Text content, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn events_default_limit_50() {
    let tmp = TempDir::new().unwrap();
    let tool = setup_instance(tmp.path(), "inst-1");

    let resp = call_text(
        &tool,
        json!({
            "instance": "inst-1",
        }),
    )
    .await;

    assert_eq!(resp["instance_dir"], "inst-1");
    assert_eq!(resp["offset"], 0);
    assert_eq!(resp["events_limit"], 50);
    assert_eq!(resp["total_matching"], 20);
    assert!(resp["next_offset"].is_null());
    assert_eq!(resp["events"].as_array().unwrap().len(), 20);
    tmp.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn events_limit_clamped_to_500() {
    let tmp = TempDir::new().unwrap();
    let tool = setup_instance(tmp.path(), "inst-clamp");

    let resp = call_text(
        &tool,
        json!({
            "instance": "inst-clamp",
            "events_limit": 99999,
        }),
    )
    .await;

    assert_eq!(resp["events_limit"], 500);
    assert_eq!(resp["total_matching"], 20);
    assert!(resp["next_offset"].is_null());
    assert_eq!(resp["events"].as_array().unwrap().len(), 20);
    tmp.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn events_offset_skips_matching() {
    let tmp = TempDir::new().unwrap();
    let tool = setup_instance(tmp.path(), "inst-offset");

    let resp = call_text(
        &tool,
        json!({
            "instance": "inst-offset",
            "offset": 2,
        }),
    )
    .await;

    assert_eq!(resp["offset"], 2);
    assert_eq!(resp["total_matching"], 20);
    assert_eq!(resp["events"].as_array().unwrap().len(), 18);
    assert!(resp["next_offset"].is_null());
    tmp.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn events_type_filter_includes_only_matching() {
    let tmp = TempDir::new().unwrap();
    let tool = setup_instance(tmp.path(), "inst-types");

    let resp = call_text(
        &tool,
        json!({
            "instance": "inst-types",
            "types": ["agent_started"],
        }),
    )
    .await;

    assert_eq!(resp["total_matching"], 4);
    let events = resp["events"].as_array().unwrap();
    assert_eq!(events.len(), 4);
    for ev in events {
        assert_eq!(ev["type"], "agent_started");
    }
    assert!(resp["next_offset"].is_null());
    tmp.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn events_type_filter_with_multiple_types() {
    let tmp = TempDir::new().unwrap();
    let tool = setup_instance(tmp.path(), "inst-multi");

    let resp = call_text(
        &tool,
        json!({
            "instance": "inst-multi",
            "types": ["agent_started", "agent_done"],
        }),
    )
    .await;

    assert_eq!(resp["total_matching"], 8);
    let events = resp["events"].as_array().unwrap();
    assert_eq!(events.len(), 8);
    for ev in events {
        let t = ev["type"].as_str().unwrap();
        assert!(t == "agent_started" || t == "agent_done");
    }
    tmp.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn events_agent_filter_includes_only_matching_agent() {
    let tmp = TempDir::new().unwrap();
    let tool = setup_instance(tmp.path(), "inst-agent");

    let resp = call_text(
        &tool,
        json!({
            "instance": "inst-agent",
            "agent_id": "a1",
        }),
    )
    .await;

    assert_eq!(resp["total_matching"], 6);
    let events = resp["events"].as_array().unwrap();
    assert_eq!(events.len(), 6);
    for ev in events {
        assert_eq!(ev["agent_id"], "a1");
    }
    assert!(resp["next_offset"].is_null());
    tmp.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn events_next_offset_null_on_last_page() {
    let tmp = TempDir::new().unwrap();
    let tool = setup_instance(tmp.path(), "inst-last");

    let resp = call_text(
        &tool,
        json!({
            "instance": "inst-last",
            "offset": 15,
        }),
    )
    .await;

    assert_eq!(resp["total_matching"], 20);
    assert_eq!(resp["events"].as_array().unwrap().len(), 5);
    assert!(
        resp["next_offset"].is_null(),
        "last page must have next_offset=null"
    );
    tmp.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn events_next_offset_set_when_more_remain() {
    let tmp = TempDir::new().unwrap();
    let tool = setup_instance(tmp.path(), "inst-next");

    let resp = call_text(
        &tool,
        json!({
            "instance": "inst-next",
            "offset": 10,
            "events_limit": 5,
        }),
    )
    .await;

    assert_eq!(resp["total_matching"], 20);
    assert_eq!(resp["events"].as_array().unwrap().len(), 5);
    assert_eq!(resp["next_offset"], json!(15));
    tmp.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn events_missing_instance_invalid_input() {
    let tmp = TempDir::new().unwrap();
    let tool = build_tool(tmp.path());

    let err = tool
        .call(json!({}), None)
        .await
        .expect_err("missing instance must error");

    match err {
        ToolSourceError::InvalidInput(msg) => {
            assert!(
                msg.contains("'instance'"),
                "error message should mention instance_dir, got: {msg}"
            );
        }
        other => panic!("expected InvalidInput, got: {other:?}"),
    }
    tmp.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn events_missing_events_jsonl_returns_empty_array() {
    let tmp = TempDir::new().unwrap();
    let inst_path = tmp.path().join(".anureo").join("instances").join("ghost");
    fs::create_dir_all(&inst_path).unwrap();

    let tool = build_tool(tmp.path());

    let resp = call_text(
        &tool,
        json!({
            "instance": "ghost",
        }),
    )
    .await;

    assert_eq!(resp["instance_dir"], "ghost");
    assert_eq!(resp["offset"], 0);
    assert_eq!(resp["events_limit"], 50);
    assert_eq!(resp["total_matching"], 0);
    assert!(resp["next_offset"].is_null());
    assert!(resp["events"].as_array().unwrap().is_empty());
    tmp.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn events_unparseable_line_skipped_silently() {
    let tmp = TempDir::new().unwrap();
    let mut body = synthetic_events_jsonl();
    body.push_str("this-is-not-valid-json\n");
    body.push_str("{also bad json\n");
    body.push('\n');

    let inst_path = tmp.path().join(".anureo").join("instances").join("noisy");
    fs::create_dir_all(&inst_path).unwrap();
    fs::write(inst_path.join("events.jsonl"), body).unwrap();

    let tool = build_tool(tmp.path());

    let resp = call_text(
        &tool,
        json!({
            "instance": "noisy",
        }),
    )
    .await;

    assert_eq!(
        resp["total_matching"], 20,
        "garbage + blank lines must not be counted"
    );
    let events = resp["events"].as_array().unwrap();
    assert_eq!(events.len(), 20);
    for ev in events {
        assert!(
            ev.get("type").is_some(),
            "malformed entries must not appear in events"
        );
    }
    tmp.close().unwrap();
}
