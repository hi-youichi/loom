use agent::agent::AgentConfig;
use serde_json::{json, Value};
use std::path::Path;
use tool_core::{Tool, ToolCallContent};
use tool_workflow::WorkflowTool;

fn tool_for(working_folder: &Path) -> WorkflowTool {
    let mut config = AgentConfig::default();
    config.working_folder = Some(working_folder.to_path_buf());
    WorkflowTool::new(config)
}

fn parse_text(content: ToolCallContent) -> Value {
    let ToolCallContent::Text(text) = content else {
        panic!("workflow tool should return text content");
    };
    serde_json::from_str(&text).expect("workflow tool should return JSON")
}

#[tokio::test]
async fn legacy_run_executes_and_returns_deprecation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tool = tool_for(temp.path());

    let content = tool
        .call(
            json!({
                "action": "run",
                "script": "function main() report({ok = true}) end",
            }),
            None,
        )
        .await
        .expect("legacy run action should execute");
    let value = parse_text(content);

    assert_eq!(value["ok"], true);
    assert_eq!(
        value["deprecation"],
        "run is now execute; update your calls."
    );

    let instances_dir = temp.path().join(".loom").join("instances");
    let instance_dirs: Vec<_> = std::fs::read_dir(&instances_dir)
        .expect("instances directory should exist")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .collect();
    assert_eq!(instance_dirs.len(), 1);
    assert!(instance_dirs[0]
        .file_name()
        .to_string_lossy()
        .starts_with("loom-instance_"));
    assert!(instance_dirs[0].path().join("checkpoint.json").is_file());
}

#[tokio::test]
async fn legacy_list_runs_lists_instances_and_returns_deprecation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tool = tool_for(temp.path());

    let value = parse_text(
        tool.call(json!({"action": "list-runs"}), None)
            .await
            .expect("legacy list-runs action should be accepted"),
    );

    assert_eq!(value["instances"], json!([]));
    assert_eq!(
        value["deprecation"],
        "list-runs is now list-instances; update your calls."
    );
}

#[tokio::test]
async fn legacy_run_status_reads_instance_and_returns_deprecation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let instance_name = "loom-instance_fixture";
    let instance_dir = temp
        .path()
        .join(".loom")
        .join("instances")
        .join(instance_name);
    std::fs::create_dir_all(&instance_dir).expect("create instance fixture");
    std::fs::write(
        instance_dir.join("checkpoint.json"),
        r#"{"run_id":"run-fixture","status":"completed"}"#,
    )
    .expect("write checkpoint");
    std::fs::write(instance_dir.join("events.jsonl"), "").expect("write empty event stream");

    let tool = tool_for(temp.path());
    let value = parse_text(
        tool.call(
            json!({
                "action": "run-status",
                "instance_dir": instance_name,
            }),
            None,
        )
        .await
        .expect("legacy run-status action should be accepted"),
    );

    assert_eq!(value["checkpoint"]["run_id"], "run-fixture");
    assert_eq!(
        value["deprecation"],
        "run-status is now instance-summary; update your calls."
    );
}
