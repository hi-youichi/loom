use std::time::{Duration, Instant};

use agent::agent::AgentConfig;
use serde_json::Value;
use tempfile::tempdir;
use tool_core::{Tool, ToolCallContent};
use tool_workflow::{WorkflowStartTool, WorkflowStatusTool};

fn text(content: ToolCallContent) -> String {
    match content {
        ToolCallContent::Text(value) => value,
        other => panic!("expected text, got {other:?}"),
    }
}

#[tokio::test]
async fn start_returns_before_terminal_summary_is_available() {
    let temp = tempdir().expect("tempdir");
    let config = AgentConfig {
        working_folder: Some(temp.path().to_path_buf()),
        ..AgentConfig::default()
    };
    let start = WorkflowStartTool::new(config.clone());
    let started_at = Instant::now();
    let result = start
        .call(
            serde_json::json!({
                "script": "function main() report({result = 'ok'}) end"
            }),
            None,
        )
        .await
        .expect("workflow start");
    assert!(started_at.elapsed() < Duration::from_secs(1));

    let receipt: Value = serde_json::from_str(&text(result)).expect("receipt json");
    assert_eq!(receipt["status"], "running");
    let instance_dir = receipt["instance_dir"].as_str().expect("instance_dir");

    let status = WorkflowStatusTool::new(config);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = status
            .call(serde_json::json!({"instance_dir": instance_dir}), None)
            .await
            .expect("workflow status");
        let value: Value = serde_json::from_str(&text(result)).expect("status json");
        if value["status"] != "running" {
            assert_eq!(value["status"], "completed");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "workflow did not reach a terminal state"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
