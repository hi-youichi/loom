use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use tool_core::tool_name::TOOL_WORKFLOW_RESUME;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

use crate::runtime::WorkflowRuntime;

pub struct WorkflowResumeTool {
    pub(crate) runtime: Arc<WorkflowRuntime>,
}

impl WorkflowResumeTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

#[async_trait]
impl Tool for WorkflowResumeTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_RESUME
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_RESUME.to_string(),
            description: Some(
                "Resume a crashed or interrupted workflow instance. Reads the prior \
                 instance's checkpoint and starts a new run that skips \
                 already-completed agents (via journal cache) and reuses \
                 sub-agent conversation history (via thread_id + SqliteSaver).\n\n\
                 Provide:\n\
                 - instance_dir: the run directory name of the crashed instance.\n\n\
                 This tool never blocks — use `workflow_status` to follow progress."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "instance_dir": {
                        "type": "string",
                        "description": "The run directory name of the crashed instance (e.g. 'deep-research_1783957281')."
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
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        crate::service::resume_workflow(&self.runtime, args, ctx).await
    }
}
