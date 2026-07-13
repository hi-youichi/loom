//! Integration tests for `register_file_tools` — specifically the `is_background_review`
//! routing of `SkillManagerTool` factory selection (issue 011-02).
//!
//! Verifies the end-to-end behavior: when `is_background_review=true`, the registered
//! `skill_manage` tool calls `mark_agent_created` on the `SkillUsageStore` after a
//! successful create (see `SkillManagerTool::handle_create` at manage.rs:219).

use serde_json::json;
use tempfile::TempDir;
use tool_basic::register_file_tools;
use tool_core::ToolRegistryLocked;

fn make_registry(is_bg: bool) -> (TempDir, ToolRegistryLocked) {
    let dir = tempfile::tempdir().expect("tempdir");
    let wf = dir.path().canonicalize().expect("canonicalize");
    let aggregate = ToolRegistryLocked::new();
    register_file_tools(&aggregate, &wf, None, None, is_bg).expect("register_file_tools");
    (dir, aggregate)
}

fn read_usage_created_by(usage_path: &std::path::Path, name: &str) -> Option<String> {
    if !usage_path.exists() {
        return None;
    }
    let raw = std::fs::read_to_string(usage_path).expect("read usage.json");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("parse usage.json");
    value
        .get(name)
        .and_then(|e| e.get("created_by"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

// `SkillUsageStore::new(base_dir)` writes to `<base_dir>/.usage.json` (hidden file).
fn usage_path_for(wf: &std::path::Path) -> std::path::PathBuf {
    wf.join(".loom").join("skills").join(".usage.json")
}

fn create_test_skill() -> String {
    "---\nname: test-skill\ndescription: safe skill for integration test\n---\n\n# Test Skill\nsafe body content\n".to_string()
}

#[test]
fn register_with_background_review_marks_skill_as_agent_created() {
    let (dir, aggregate) = make_registry(true);
    let wf = dir.path().canonicalize().unwrap();
    let usage_path = usage_path_for(&wf);

    let result = futures_executor::block_on(aggregate.call_tool(
        "skill_manage",
        json!({"action": "create", "name": "test-skill", "content": create_test_skill()}),
        None,
    ));
    assert!(result.is_ok(), "create call should succeed: {:?}", result);

    let created_by = read_usage_created_by(&usage_path, "test-skill");
    assert_eq!(
        created_by.as_deref(),
        Some("agent"),
        "is_background_review=true: skill must be marked agent-created (usage.json at {:?})",
        usage_path
    );
}

#[test]
fn register_with_foreground_does_not_mark_skill_as_agent_created() {
    let (dir, aggregate) = make_registry(false);
    let wf = dir.path().canonicalize().unwrap();
    let usage_path = usage_path_for(&wf);

    let result = futures_executor::block_on(aggregate.call_tool(
        "skill_manage",
        json!({"action": "create", "name": "test-skill", "content": create_test_skill()}),
        None,
    ));
    assert!(result.is_ok(), "create call should succeed: {:?}", result);

    let created_by = read_usage_created_by(&usage_path, "test-skill");
    assert!(
        created_by.is_none() || created_by.as_deref() != Some("agent"),
        "is_background_review=false: skill must NOT be marked agent-created, got {:?}",
        created_by
    );
}

#[test]
fn register_creates_skill_manage_tool_in_registry() {
    let (_dir, aggregate) = make_registry(false);
    let tools = futures_executor::block_on(aggregate.list_tools());
    assert!(
        tools.iter().any(|t| t.name == "skill_manage"),
        "skill_manage tool must always be registered by register_file_tools"
    );
}
