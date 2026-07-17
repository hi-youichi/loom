//! Schema contract tests for the six workflow_* tools.
//!
//! Migrated from the legacy `WorkflowTool` action-dispatch schema. The new
//! design exposes six specialised LLM-facing tools; the schema test asserts
//! each is registered under its expected name with the right output hint,
//! and that the legacy action-dispatch enum is gone.

use std::path::Path;

use agent::agent::AgentConfig;
use tool_core::tool_name::{
    TOOL_WORKFLOW_EVENTS, TOOL_WORKFLOW_FILES, TOOL_WORKFLOW_LIST, TOOL_WORKFLOW_SOURCE,
    TOOL_WORKFLOW_START, TOOL_WORKFLOW_STATUS,
};
use tool_core::{Tool, ToolCallContent};
use tool_workflow::{
    WorkflowEventsTool, WorkflowFilesTool, WorkflowListTool, WorkflowSourceTool, WorkflowStartTool,
    WorkflowStatusTool,
};

fn make_tools(working_folder: &Path) -> [Box<dyn Tool>; 6] {
    let cfg = AgentConfig {
        working_folder: Some(working_folder.to_path_buf()),
        ..Default::default()
    };
    [
        Box::new(WorkflowStartTool::new(cfg.clone())) as Box<dyn Tool>,
        Box::new(WorkflowStatusTool::new(cfg.clone())) as Box<dyn Tool>,
        Box::new(WorkflowListTool::new(cfg.clone())) as Box<dyn Tool>,
        Box::new(WorkflowEventsTool::new(cfg.clone())) as Box<dyn Tool>,
        Box::new(WorkflowSourceTool::new(cfg.clone())) as Box<dyn Tool>,
        Box::new(WorkflowFilesTool::new(cfg)) as Box<dyn Tool>,
    ]
}

#[test]
fn six_tool_names_and_input_schemas_match_constants() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tools = make_tools(temp.path());

    let expected: [(&str, Option<&str>); 6] = [
        (TOOL_WORKFLOW_START, Some("script")),
        (TOOL_WORKFLOW_STATUS, Some("instance")),
        (TOOL_WORKFLOW_LIST, Some("limit")),
        (TOOL_WORKFLOW_EVENTS, Some("instance")),
        (TOOL_WORKFLOW_SOURCE, Some("instance")),
        (TOOL_WORKFLOW_FILES, None),
    ];

    for (tool, (name, marker)) in tools.iter().zip(expected.iter()) {
        assert_eq!(tool.name(), *name, "tool name mismatch");
        let spec = tool.spec();
        assert_eq!(spec.name, *name);
        if let Some(marker) = marker {
            assert!(
                spec.input_schema["properties"]
                    .as_object()
                    .expect("properties object")
                    .contains_key(*marker),
                "schema for {name} must advertise property '{marker}'"
            );
        }
        let legacy = spec.input_schema["properties"]
            .get("action")
            .map(|v| v.to_string())
            .unwrap_or_default();
        assert!(
            !legacy.contains("execute") && !legacy.contains("instance-summary"),
            "schema for {name} must not carry legacy 'action' enum; got {legacy}"
        );
    }
}

#[test]
fn workflow_list_walks_both_current_and_legacy_instance_dirs() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    rt.block_on(async {
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
        std::fs::write(workflows_dir.join("local.lua"), "function main() end")
            .expect("write workflow");
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

        let cfg = AgentConfig {
            working_folder: Some(temp.path().to_path_buf()),
            ..Default::default()
        };
        let tool = WorkflowListTool::new(cfg);

        let content = tool
            .call(serde_json::json!({}), None)
            .await
            .expect("list instances");
        let ToolCallContent::Text(text) = content else {
            panic!("expected text content");
        };
        let v: serde_json::Value = serde_json::from_str(&text).expect("workflow_list returns JSON");

        assert_eq!(v["count"], 2);
        assert_eq!(v["instances"][0]["instance_dir"], "loom-instance_current");
        assert_eq!(v["instances"][0]["instance_id"], "current");
        assert_eq!(v["instances"][1]["instance_dir"], "luft-workflow_old");
        assert_eq!(v["instances"][1]["instance_id"], "old");
    });
}
