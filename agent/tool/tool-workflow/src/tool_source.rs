use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use tool_core::tool_name::TOOL_WORKFLOW_SOURCE;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

use crate::runtime::WorkflowRuntime;

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
        crate::service::read_source(&self.runtime, &args)
    }
}
