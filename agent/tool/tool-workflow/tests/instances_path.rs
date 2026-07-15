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
async fn loom_paths_are_primary_for_workflows_and_instances() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workflows_dir = temp.path().join(".loom").join("workflows");
    let instances_dir = temp.path().join(".loom").join("instances");
    let current_instance = instances_dir.join("loom-instance_current");
    let old_instance = temp
        .path()
        .join(".luft")
        .join("runs")
        .join("luft-workflow_old");

    std::fs::create_dir_all(&workflows_dir).expect("create workflows directory");
    std::fs::write(workflows_dir.join("local.lua"), "function main() end").expect("write workflow");
    std::fs::create_dir_all(&current_instance).expect("create current instance");
    std::fs::write(
        current_instance.join("checkpoint.json"),
        r#"{"run_id":"current","status":"completed","created_at":2}"#,
    )
    .expect("write current checkpoint");
    std::fs::create_dir_all(&old_instance).expect("create old instance");
    std::fs::write(
        old_instance.join("checkpoint.json"),
        r#"{"run_id":"old","status":"completed","created_at":1}"#,
    )
    .expect("write old checkpoint");

    let tool = tool_for(temp.path());
    let workflows = parse_text(
        tool.call(json!({"action": "list-workflows"}), None)
            .await
            .expect("list workflows"),
    );
    assert_eq!(workflows["count"], 1);
    assert_eq!(workflows["workflows"][0]["name"], "local");
    assert_eq!(
        Path::new(workflows["directory"].as_str().expect("directory string")),
        workflows_dir
    );

    let instances = parse_text(
        tool.call(json!({"action": "list-instances"}), None)
            .await
            .expect("list instances"),
    );
    // T-04: `list-instances` walks BOTH `.loom/instances/` (current
    // layout, post-T-02) AND `.luft/runs/` (legacy layout). The test
    // seeds one entry under each root, so the listing carries TWO
    // entries (paginated by `created_at` DESC). Renamed from `run_id`
    // -> `instance_id` because T-04 unifies the field across both
    // source directories.
    assert_eq!(instances["count"], 2);
    assert_eq!(
        instances["instances"][0]["instance_dir"],
        "loom-instance_current"
    );
    assert_eq!(instances["instances"][0]["instance_id"], "current");
    assert_eq!(
        instances["instances"][1]["instance_dir"],
        "luft-workflow_old"
    );
    assert_eq!(instances["instances"][1]["instance_id"], "old");
}

#[test]
fn schema_advertises_only_new_action_names() {
    let temp = tempfile::tempdir().expect("tempdir");
    let spec = tool_for(temp.path()).spec();
    let action = &spec.input_schema["properties"]["action"];

    assert_eq!(
        action["enum"],
        json!([
            "execute",
            "list-workflows",
            "list-instances",
            "instance-summary",
            "instance-events",
            "instance-source"
        ])
    );
    assert_eq!(action["default"], "execute");
    assert!(spec.input_schema["properties"]
        .get("instance_dir")
        .is_some());
    assert!(spec.input_schema["properties"].get("run_dir").is_none());
}
