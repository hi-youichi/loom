//! AgentTool: exposes a ReactRunner as a Tool that other agents can call.
//!
//! Use [`ReactRunner::as_tool`] to convert a named runner into an `AgentTool`,
//! then register it in another agent's `AggregateToolSource`. The parent agent's
//! LLM will see it as a regular tool and can delegate tasks to it.
//!
//! # Input schema
//!
//! ```json
//! { "task": "<natural-language task description>" }
//! ```
//!
//! The tool runs the full ReAct loop and returns the sub-agent's final reply.

use std::sync::Arc;

use async_trait::async_trait;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;

use super::runner::ReactRunner;

/// A [`Tool`] wrapping a [`ReactRunner`], allowing other agents to delegate tasks.
///
/// Created via [`ReactRunner::as_tool`]. The tool name is taken from
/// `runner.name` and the description from `runner.description`.
pub struct AgentTool {
    pub(super) runner: Arc<ReactRunner>,
}

impl AgentTool {
    /// The tool name used in LLM calls (snake_case, no spaces).
    pub fn tool_name(&self) -> String {
        // Replace spaces/hyphens with underscores so it's a valid tool identifier.
        self.runner
            .name
            .as_deref()
            .unwrap_or("agent")
            .replace([' ', '-'], "_")
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        // SAFETY: `as_tool()` asserts name is Some before constructing AgentTool.
        self.runner.name.as_deref().unwrap_or("agent")
    }

    fn spec(&self) -> ToolSpec {
        let name = self.tool_name();
        let description = self.runner.description.clone().unwrap_or_else(|| {
            format!("Delegate a task to the {} agent.", name)
        });

        ToolSpec {
            name,
            description: Some(description),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The task or question to delegate to this agent. Provide full context; the agent has no memory of the current conversation."
                    }
                },
                "required": ["task"]
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let task = args
            .get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing required argument: task".into()))?;

        let final_state = self
            .runner
            .invoke(task)
            .await
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;

        let reply = final_state
            .last_assistant_reply()
            .unwrap_or_else(|| "(no reply)".to_string());

        Ok(ToolCallContent { text: reply })
    }
}
