use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use tool_core::tool_name::TOOL_WORKFLOW_CANCEL;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

use crate::runtime::WorkflowRuntime;

pub struct WorkflowCancelTool {
    pub(crate) runtime: Arc<WorkflowRuntime>,
}

impl WorkflowCancelTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

#[async_trait]
impl Tool for WorkflowCancelTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_CANCEL
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_CANCEL.to_string(),
            description: Some(
                "Signal cancellation for a running workflow instance. The in-flight agent \
                 (if any) finishes its current turn and returns a Cancelled error; the \
                 checkpoint is then marked \"cancelled\" instead of \"completed\". Use \
                 `workflow_status` after cancelling to observe the terminal state.\n\n\
                 Provides:\n\
                 - instance: directory name returned by workflow_start.\n\n\
                 The lookup targets the same in-memory registry that workflow_start uses, \
                 so cancel works only for runs owned by this process. After the run \
                 reaches a terminal state (or was owned by another process), the tool \
                 returns result=\"not_found_or_terminal\"."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "instance": {
                        "type": "string",
                        "description": "Instance directory name returned by workflow_start."
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
        crate::service::cancel_workflow(&self.runtime, &args)
    }
}
