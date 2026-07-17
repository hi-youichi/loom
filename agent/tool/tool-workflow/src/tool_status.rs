use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use tool_core::tool_name::TOOL_WORKFLOW_STATUS;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
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
                    "instance": {
                        "type": "string",
                        "description": "Instance directory name returned by workflow_start or workflow_list."
                    }
                },
                "required": ["instance"]
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        crate::service::read_status(&self.runtime, &args).await
    }
}
