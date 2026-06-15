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

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use tool_core::Tool;

use super::runner::ReactRunner;

/// A [`Tool`] wrapping a [`ReactRunner`], allowing other agents to delegate tasks.
///
/// Created via [`ReactRunner::as_tool`]. The tool name is taken from
/// `runner.name` and the description from `runner.description`.
#[allow(dead_code)]
pub struct AgentTool {
    pub(super) runner: Arc<ReactRunner>,
}

impl AgentTool {
    /// The tool name used in LLM calls (snake_case, no spaces).
    #[allow(dead_code)]
    pub fn tool_name(&self) -> String {
        // Default to "agent" since ReactRunner doesn't have a name field
        "agent".replace([' ', '-'], "_")
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "agent"
    }

    fn spec(&self) -> ToolSpec {
        let name = self.tool_name();
        let description = "Run a sub-agent task".to_string();
        let input_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Natural-language description of the task to delegate"
                }
            },
            "required": ["task"]
        });
        ToolSpec {
            name,
            description: Some(description),
            input_schema,
            output_hint: None,
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

        let outcome = self
            .runner
            .stream_with_callback(task, Some(|_: loom_stream::StreamEvent<loom_types::state::ReActState>| {}))
            .await
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;

        let reply = match outcome {
            crate::runner_common::StreamRunOutcome::Finished(s) => {
                s.last_assistant_reply().unwrap_or_else(|| "(no reply)".to_string())
            }
            crate::runner_common::StreamRunOutcome::Cancelled => "(sub-agent cancelled)".to_string(),
        };

        Ok(ToolCallContent::text(reply))
    }
}