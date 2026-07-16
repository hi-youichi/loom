use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use tool_core::tool_name::TOOL_WORKFLOW_STATUS;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

use crate::common::{
    instance_dir_arg, is_terminal_checkpoint, running_receipt, sanitize_instance_for_public,
};
use crate::runtime::WorkflowRuntime;

pub struct WorkflowStatusTool {
    pub(crate) runtime: Arc<WorkflowRuntime>,
}

impl WorkflowStatusTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

#[async_trait]
impl Tool for WorkflowStatusTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_STATUS
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_STATUS.to_string(),
            description: Some(
                "Read the current state of a workflow instance. Returns status=running \
                 while it is active, or a complete terminal summary after it finishes. \
                 The summary includes agent results, token usage, phase timing, event \
                 statistics, and a bounded report preview. Internal references are \
                 never returned. Sleep between status checks instead of polling in a \
                 tight loop. Use `workflow_events` for detailed execution events or \
                 `workflow_source` for the executed Lua source."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "instance_dir": {
                        "type": "string",
                        "description": "Instance directory name returned by workflow_start or workflow_list."
                    }
                },
                "required": ["instance_dir"]
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let dir = instance_dir_arg(&args, "workflow_status")?;
        let new_path = self.runtime.instances_root().join(dir);
        let legacy_path = self.runtime.runs_root().join(dir);

        let resolved = if new_path.is_dir() {
            new_path.clone()
        } else if legacy_path.is_dir() {
            legacy_path.clone()
        } else {
            return Err(ToolSourceError::InvalidInput(format!(
                "Instance '{dir}' not found"
            )));
        };

        let instance_json_path = resolved.join("instance.json");
        if instance_json_path.is_file() {
            let raw = std::fs::read_to_string(&instance_json_path).map_err(|e| {
                ToolSourceError::ToolError(format!("Failed to read instance summary: {e}"))
            })?;
            let value: Value = serde_json::from_str(&raw).map_err(|e| {
                ToolSourceError::ToolError(format!("Invalid instance summary: {e}"))
            })?;
            let sanitized = sanitize_instance_for_public(value);
            return Ok(ToolCallContent::Text(
                serde_json::to_string_pretty(&sanitized).unwrap_or_default(),
            ));
        }

        let checkpoint_path = resolved.join("checkpoint.json");

        if checkpoint_path.is_file() {
            let should_rebuild = if resolved == new_path {
                is_terminal_checkpoint(&checkpoint_path).unwrap_or(false)
            } else {
                true
            };
            if should_rebuild {
                return self.runtime.rebuild_summary(dir, &checkpoint_path).await;
            }
        }

        if resolved == new_path {
            return Ok(running_receipt(dir));
        }

        Err(ToolSourceError::ToolError(format!(
            "Instance '{dir}' is incomplete (missing checkpoint)"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::agent::AgentConfig;
    use std::path::Path;

    fn runtime_with(folder: &Path) -> Arc<WorkflowRuntime> {
        let cfg = AgentConfig {
            working_folder: Some(folder.to_path_buf()),
            ..Default::default()
        };
        Arc::new(WorkflowRuntime::new(cfg))
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(f)
    }

    #[test]
    fn status_returns_sanitized_view_when_instance_json_present() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "loom-instance_x";
        let dir_path = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join(instance_dir);
        std::fs::create_dir_all(&dir_path).unwrap();
        let raw = json!({
            "schema_version": 1,
            "instance_id": "run-x",
            "instance_dir": instance_dir,
            "workflow": {"kind": "file", "name": "wf", "path": "/secret/abs"},
            "agents": [
                {"agent_id": "a", "output_ref": "agent-outputs/a.txt", "output_size": 4096}
            ],
            "report": {"ref": "report.json", "preview": "hi", "value_type": "object", "size_bytes": 5},
            "checkpoint_hash": "deadbeef",
            "status": "completed",
        });
        std::fs::write(
            dir_path.join("instance.json"),
            serde_json::to_string_pretty(&raw).unwrap(),
        )
        .unwrap();

        let rt = runtime_with(tmp.path());
        let tool = WorkflowStatusTool {
            runtime: rt.clone(),
        };
        let result = block_on(tool.call(json!({"instance_dir": instance_dir}), None)).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        assert!(v["workflow"].get("path").is_none());
        assert!(v["agents"][0].get("output_ref").is_none());
        assert!(v["report"].get("ref").is_none());
        assert!(v.get("checkpoint_hash").is_none());
        assert_eq!(v["status"], "completed");
    }

    #[test]
    fn status_returns_running_when_only_dir_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "loom-instance_running";
        let dir_path = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join(instance_dir);
        std::fs::create_dir_all(&dir_path).unwrap();

        let rt = runtime_with(tmp.path());
        let tool = WorkflowStatusTool { runtime: rt };
        let result = block_on(tool.call(json!({"instance_dir": instance_dir}), None)).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["status"], "running");
    }

    #[test]
    fn status_errors_on_legacy_dir_without_checkpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "legacy-blank";
        let dir_path = tmp.path().join(".luft").join("runs").join(instance_dir);
        std::fs::create_dir_all(&dir_path).unwrap();

        let rt = runtime_with(tmp.path());
        let tool = WorkflowStatusTool { runtime: rt };
        let err = block_on(tool.call(json!({"instance_dir": instance_dir}), None)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("corrupt") || msg.contains("missing checkpoint"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn status_errors_on_unknown_instance_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = runtime_with(tmp.path());
        let tool = WorkflowStatusTool { runtime: rt };
        let err = block_on(tool.call(json!({"instance_dir": "does-not-exist"}), None)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("not found"));
    }
}