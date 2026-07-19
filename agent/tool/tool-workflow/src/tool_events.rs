use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use tool_core::tool_name::TOOL_WORKFLOW_EVENTS;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

use crate::runtime::WorkflowRuntime;

pub struct WorkflowEventsTool {
    pub(crate) runtime: Arc<WorkflowRuntime>,
}

impl WorkflowEventsTool {
    pub fn new(runtime: Arc<WorkflowRuntime>) -> Self { Self { runtime } }
}

#[async_trait]
impl Tool for WorkflowEventsTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_EVENTS
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_EVENTS.to_string(),
            description: Some(
                "Paginated, filtered access to the structured event stream of a \
                 workflow instance. Filters: `types` (array of event-type strings) \
                 and `agent_id`. Pagination: `offset` (skip N matching events) and \
                 `events_limit` (page size, 1..=500)."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "instance": {
                        "type": "string",
                        "description": "Instance directory name returned by workflow_start or workflow_list."
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "default": 0,
                        "description": "Skip the first N matching events."
                    },
                    "events_limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 500,
                        "default": 50,
                        "description": "Page size (clamped to 500)."
                    },
                    "types": {
                        "type": ["array", "null"],
                        "items": {"type": "string"},
                        "description": "Restrict returned events to those whose `type` field is in this set."
                    },
                    "agent_id": {
                        "type": ["string", "null"],
                        "description": "Restrict returned events to those with this `agent_id`."
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
        crate::service::read_events(&self.runtime, &args)
    }
}
