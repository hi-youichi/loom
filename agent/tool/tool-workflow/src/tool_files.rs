use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use tool_core::tool_name::TOOL_WORKFLOW_FILES;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

use crate::runtime::WorkflowRuntime;

pub struct WorkflowFilesTool {
    pub(crate) runtime: Arc<WorkflowRuntime>,
}

impl WorkflowFilesTool {
    pub fn new(runtime: Arc<WorkflowRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for WorkflowFilesTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_FILES
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_FILES.to_string(),
            description: Some(
                "List the available Lua workflow definitions. Returns each \
                 workflow's name. Pass a returned name to `workflow_start`."
                    .to_string(),
            ),
            input_schema: json!({"type": "object", "properties": {}}),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        _args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        crate::service::list_workflow_files(&self.runtime)
    }
}
