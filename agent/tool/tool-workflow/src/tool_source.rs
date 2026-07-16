use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use tool_core::tool_name::TOOL_WORKFLOW_SOURCE;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

use crate::common::{instance_dir_arg, truncate_for_preview};
use crate::runtime::WorkflowRuntime;

const DEFAULT_SOURCE_PREVIEW_LIMIT: usize = 8_192;

pub struct WorkflowSourceTool {
    pub(crate) runtime: Arc<WorkflowRuntime>,
}

impl WorkflowSourceTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

#[async_trait]
impl Tool for WorkflowSourceTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_SOURCE
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_SOURCE.to_string(),
            description: Some(
                "Preview the Lua source backing a workflow instance. The source \
                 is truncated to a bounded preview; no path is exposed."
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
        read_source(&self.runtime, &args)
    }
}

fn read_source(
    runtime: &WorkflowRuntime,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let dir = instance_dir_arg(args, "workflow_source")?;
    let resolved = runtime
        .resolve_instance_path(dir)
        .ok_or_else(|| ToolSourceError::InvalidInput(format!("Instance '{dir}' not found")))?;

    let source = std::fs::read_to_string(resolved.join("workflow.lua")).map_err(|e| {
        ToolSourceError::ToolError(format!("Failed to read workflow source: {e}"))
    })?;

    let (preview, truncated) = if source.len() > DEFAULT_SOURCE_PREVIEW_LIMIT {
        (
            truncate_for_preview(&source, DEFAULT_SOURCE_PREVIEW_LIMIT),
            true,
        )
    } else {
        (source.clone(), false)
    };

    Ok(ToolCallContent::Text(
        serde_json::to_string_pretty(&json!({
            "instance_dir": dir,
            "workflow_source": preview,
            "truncated": truncated,
        }))
        .unwrap_or_default(),
    ))
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

    #[test]
    fn source_returns_short_workflow_preview_untruncated() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "loom-instance_short";
        let dir = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join(instance_dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("workflow.lua"), "function main() end").unwrap();

        let rt = runtime_with(tmp.path());
        let result = read_source(&rt, &json!({"instance_dir": instance_dir})).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["workflow_source"], "function main() end");
        assert_eq!(v["truncated"], false);
    }

    #[test]
    fn source_truncates_long_workflow_to_preview_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "loom-instance_long";
        let dir = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join(instance_dir);
        std::fs::create_dir_all(&dir).unwrap();
        let big = "x".repeat(DEFAULT_SOURCE_PREVIEW_LIMIT + 40);
        std::fs::write(dir.join("workflow.lua"), &big).unwrap();

        let rt = runtime_with(tmp.path());
        let result = read_source(&rt, &json!({"instance_dir": instance_dir})).unwrap();
        let text = match result {
            ToolCallContent::Text(s) => s,
            _ => panic!("expected text output"),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        let preview = v["workflow_source"].as_str().unwrap();
        assert!(preview.ends_with('…'));
        assert_eq!(v["truncated"], true);
        assert!(preview.len() <= DEFAULT_SOURCE_PREVIEW_LIMIT + '…'.len_utf8());
    }

    #[test]
    fn source_errors_when_missing_workflow_lua() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = "loom-instance_empty";
        let dir = tmp
            .path()
            .join(".loom")
            .join("instances")
            .join(instance_dir);
        std::fs::create_dir_all(&dir).unwrap();

        let rt = runtime_with(tmp.path());
        let err = read_source(&rt, &json!({"instance_dir": instance_dir})).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("Failed to read workflow source"));
    }

    #[test]
    fn source_errors_on_unknown_instance_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = runtime_with(tmp.path());
        let err = read_source(&rt, &json!({"instance_dir": "no-such-thing"})).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("not found"));
    }
}