//! AgentCancelTool: abort a running background agent.

use async_trait::async_trait;
use serde_json::Value;
use tool_core::{
    tool_name::TOOL_AGENT_CANCEL, Tool, ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec,
};

use crate::tools::agent::registry::AsyncAgentRegistry;

pub struct AgentCancelTool {
    registry: AsyncAgentRegistry,
}

impl AgentCancelTool {
    pub fn new(registry: AsyncAgentRegistry) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for AgentCancelTool {
    fn name(&self) -> &str {
        TOOL_AGENT_CANCEL
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_AGENT_CANCEL.to_string(),
            description: Some(
                "Abort a running background agent. \
                 The agent's tokio task is cancelled immediately. \
                 Already completed/failed agents cannot be cancelled."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The agent_id to cancel."
                    }
                },
                "required": ["agent_id"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let agent_id = args
            .get("agent_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolSourceError::InvalidInput("missing required argument: agent_id".into())
            })?;

        self.registry
            .cancel(agent_id)
            .map_err(ToolSourceError::InvalidInput)?;

        // Return the updated entry.
        let response = match self.registry.get(agent_id) {
            Some(entry) => entry.to_json(),
            None => serde_json::json!({ "agent_id": agent_id, "status": "cancelled" }),
        };
        Ok(ToolCallContent::text(
            serde_json::to_string_pretty(&response).unwrap(),
        ))
    }
}
