use async_trait::async_trait;

use crate::cli_run::list_available_profiles;
use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;

pub const TOOL_LIST_AGENTS: &str = "list_agents";

pub struct ListAgentsTool;

impl ListAgentsTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListAgentsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ListAgentsTool {
    fn name(&self) -> &str {
        TOOL_LIST_AGENTS
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_LIST_AGENTS.to_string(),
            description: Some(
                "List all available agent profiles that can be used with `invoke_agent`."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        _args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let profiles = list_available_profiles();
        if profiles.is_empty() {
            return Ok(ToolCallContent::text("No agents available.".to_string()));
        }
        let mut lines = Vec::with_capacity(profiles.len() + 1);
        lines.push(format!("Available agents ({}):\n", profiles.len()));
        for p in &profiles {
            let desc = p.description.as_deref().unwrap_or("(no description)");
            lines.push(format!("- {} [{}] {}", p.name, p.source, desc));
        }
        Ok(ToolCallContent::text(lines.join("\n")))
    }
}
