//! Integration smoke for the public surface of `instance.rs`.
//!
//! Reads a real instance dir (LOOM_TEST_INSTANCES_DIR), runs `build_instance_meta` +
//! `write_instance_artifacts`, then prints a structural check so we can
//! eyeball the produced JSON against the design.
//!
//! Run with:
//!   LOOM_TEST_INSTANCES_DIR=<path> cargo test -p tool-workflow \
//!       --test instance_smoke -- --nocapture

use serde_json::Value;
use std::path::PathBuf;
use tool_workflow::{build_instance_meta, write_instance_artifacts, WorkflowRef};

fn instances_dir() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("LOOM_TEST_INSTANCES_DIR") {
        let p = PathBuf::from(v);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

fn load_jsonl(p: &std::path::Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(p) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[test]
fn dump_instance_for_real_instance() {
    let Some(instances) = instances_dir() else {
        eprintln!("set LOOM_TEST_INSTANCES_DIR to enable this smoke");
        return;
    };
    let dir = instances.join("loom-instance_1783783769");
    if !dir.is_dir() {
        eprintln!("missing fixture {}", dir.display());
        return;
    }
    let bytes = std::fs::read(dir.join("checkpoint.json")).unwrap();
    let ckpt: Value = serde_json::from_slice(&bytes).unwrap();
    let events = load_jsonl(&dir.join("events.jsonl"));

    let wref = WorkflowRef {
        kind: "file",
        name: Some("hello-agents".into()),
        path: Some(".loom/workflows/hello-agents.lua".into()),
    };
    let meta = build_instance_meta(
        &ckpt,
        &events,
        None,
        &wref,
        "loom-instance_1783783769".into(),
        &bytes,
    );

    // Temporary out dir so we don't trample .loom/instances.
    let tmp = tempfile::tempdir().expect("tmpdir");
    let out = tmp.path();
    write_instance_artifacts(out, &meta, None, &[]).unwrap();

    let instance_text = std::fs::read_to_string(out.join("instance.json")).unwrap();
    let v: Value = serde_json::from_str(&instance_text).unwrap();

    // Structural assertions beyond the unit tests:
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["status"], "completed");
    assert!(!v["agents"].as_array().unwrap().is_empty());
    assert_eq!(v["checkpoint_hash"].as_str().unwrap().len(), 64);
    assert!(v["event_stats"]["total"].as_u64().unwrap() > 0);
    assert!(v["event_stats"]["by_type"]
        .as_object()
        .unwrap()
        .contains_key("agent_done"));

    eprintln!("\n========== instance.json (first 60 lines) ==========");
    for (i, line) in instance_text.lines().enumerate() {
        if i >= 60 {
            break;
        }
        eprintln!("{line}");
    }
    eprintln!(
        "========== /instance.json ({} bytes total) ==========\n",
        instance_text.len()
    );
}
