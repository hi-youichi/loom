//! AgentGetTool: query the status and result of async agent invocations.
//!
//! Usage:
//!   {"agent_id": "sub-root-dev-0-3"}   → single agent status
//!   {}                                  → list all agents

use async_trait::async_trait;
use serde_json::{json, Value};
use tool_core::{Tool, ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};

use crate::tools::agent::registry::AsyncAgentRegistry;
use tool_core::tool_name::TOOL_AGENT_GET;

pub struct AgentGetTool {
    registry: AsyncAgentRegistry,
}

impl AgentGetTool {
    pub fn new(registry: AsyncAgentRegistry) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for AgentGetTool {
    fn name(&self) -> &str {
        TOOL_AGENT_GET
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_AGENT_GET.to_string(),
            description: Some(
                "Retrieve the status or result of an async agent. \
                 Pass `agent_id` to get a specific agent, or omit to list all agents. \
                 Shares state with the `agent` tool's registry."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The agent_id returned by a background agent invocation. If omitted, lists all agents."
                    }
                }
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let agent_id = args.get("agent_id").and_then(|v| v.as_str());

        match agent_id {
            Some(id) => match self.registry.get(id) {
                Some(entry) => {
                    let response = entry.to_json();
                    Ok(ToolCallContent::text(
                        serde_json::to_string_pretty(&response).unwrap(),
                    ))
                }
                None => Err(ToolSourceError::InvalidInput(format!(
                    "agent_id '{id}' not found"
                ))),
            },
            None => {
                let entries = self.registry.list_all();
                let agents: Vec<Value> = entries.iter().map(|e| e.to_json()).collect();
                Ok(ToolCallContent::text(
                    serde_json::to_string_pretty(&json!({ "agents": agents })).unwrap(),
                ))
            }
        }
    }
}
