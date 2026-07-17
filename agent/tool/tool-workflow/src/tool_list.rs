use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use tool_core::tool_name::TOOL_WORKFLOW_LIST;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

use crate::runtime::WorkflowRuntime;

pub struct WorkflowListTool {
    pub(crate) runtime: Arc<WorkflowRuntime>,
}

impl WorkflowListTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

#[async_trait]
impl Tool for WorkflowListTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_LIST
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_LIST.to_string(),
            description: Some(
                "List completed workflow instances with optional status filtering. \
                 Results are paginated by `limit` and opaque `cursor`, and include \
                 instance identifiers, status, workflow names, timestamps, token \
                 totals, and agent counts."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "default": 20,
                        "description": "Max instances to return. Default: 20, max: 100."
                    },
                    "cursor": {
                        "type": "string",
                        "description": "Opaque cursor from a previous page's `next_cursor`."
                    },
                    "status_filter": {
                        "type": "string",
                        "enum": ["completed", "failed", "cancelled"],
                        "description": "Restrict to entries with this status. Case-insensitive."
                    }
                }
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        crate::service::list_instances(&self.runtime, &args)
    }
}
