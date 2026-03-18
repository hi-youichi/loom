//! InvokeAgentTool: dynamically invoke a sub-agent by profile name at runtime.
//!
//! Unlike `AgentTool` (which wraps a pre-built `ReactRunner`), this tool resolves
//! an agent profile by name, builds a fresh `ReactRunner`, and runs it — all at
//! call time. This lets the LLM decide which sub-agent to delegate to.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::cli_run::{build_config_from_profile, list_available_profiles, resolve_profile};
use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;
use crate::{build_react_runner, ReactBuildConfig, ToolOutputHint, ToolOutputStrategy};

pub const TOOL_INVOKE_AGENT: &str = "invoke_agent";
const DEFAULT_MAX_DEPTH: u32 = 3;

pub struct InvokeAgentTool {
    base_config: Arc<ReactBuildConfig>,
    max_depth: u32,
}

impl InvokeAgentTool {
    pub fn new(base_config: Arc<ReactBuildConfig>, max_depth: Option<u32>) -> Self {
        Self {
            base_config,
            max_depth: max_depth.unwrap_or(DEFAULT_MAX_DEPTH),
        }
    }

    fn available_agents_description(&self) -> String {
        let profiles = list_available_profiles();
        if profiles.is_empty() {
            return String::new();
        }
        let mut lines = vec![String::from("\n\nAvailable agents:")];
        for p in &profiles {
            let desc = p.description.as_deref().unwrap_or("(no description)");
            lines.push(format!("  - {}: {}", p.name, desc));
        }
        lines.join("\n")
    }
}

#[async_trait]
impl Tool for InvokeAgentTool {
    fn name(&self) -> &str {
        TOOL_INVOKE_AGENT
    }

    fn spec(&self) -> ToolSpec {
        let agents_desc = self.available_agents_description();
        let description = format!(
            "Delegate a task to another agent by profile name. The sub-agent runs a full \
             ReAct loop with its own tools and system prompt, then returns the final reply.\n\
             \n\
             Use this when a specialized agent is better suited for the sub-task. \
             Provide full context in the task parameter; the sub-agent has no memory \
             of the current conversation.{}",
            agents_desc,
        );
        ToolSpec {
            name: TOOL_INVOKE_AGENT.to_string(),
            description: Some(description),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent": {
                        "type": "string",
                        "description": "Agent profile name (e.g. 'dev', 'agent-builder') or path to profile directory."
                    },
                    "task": {
                        "type": "string",
                        "description": "Natural-language task to delegate. Include full context; the sub-agent has no memory of the current conversation."
                    },
                    "working_folder": {
                        "type": "string",
                        "description": "Optional: override working folder for the sub-agent."
                    }
                },
                "required": ["agent", "task"]
            }),
            output_hint: Some(ToolOutputHint::preferred(
                ToolOutputStrategy::SummaryOnly,
            )),
        }
    }

    async fn call(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let current_depth = ctx.map(|c| c.depth).unwrap_or(0);
        if current_depth >= self.max_depth {
            return Err(ToolSourceError::InvalidInput(format!(
                "max sub-agent depth ({}) reached; cannot invoke further agents",
                self.max_depth,
            )));
        }

        let agent_name = args.get("agent").and_then(|v| v.as_str()).ok_or_else(|| {
            ToolSourceError::InvalidInput("missing required argument: agent".into())
        })?;

        let task = args.get("task").and_then(|v| v.as_str()).ok_or_else(|| {
            ToolSourceError::InvalidInput("missing required argument: task".into())
        })?;

        let working_folder_override = args
            .get("working_folder")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from);

        let profile = resolve_profile(agent_name).map_err(|e| {
            ToolSourceError::InvalidInput(format!(
                "failed to resolve agent '{}': {}",
                agent_name, e
            ))
        })?;

        let mut sub_config = build_config_from_profile(
            &profile,
            &self.base_config,
            working_folder_override.as_deref(),
        );

        // Propagate depth + 1 so nested invoke_agent calls are tracked
        sub_config.thread_id = None;

        let runner = build_react_runner(&sub_config, None, false, None)
            .await
            .map_err(|e| {
                ToolSourceError::Transport(format!(
                    "failed to build sub-agent '{}': {}",
                    agent_name, e
                ))
            })?;

        let on_event = ctx.and_then(|c| c.stream_writer.clone()).map(|writer| {
            let agent = agent_name.to_string();
            move |event: crate::stream::StreamEvent<crate::state::ReActState>| {
                let payload = serde_json::json!({
                    "sub_agent": agent,
                    "event": format!("{:?}", event),
                });
                writer.emit_custom(payload);
            }
        });

        let final_state = runner
            .stream_with_config(task, None, on_event)
            .await
            .map_err(|e| {
                ToolSourceError::Transport(format!("sub-agent '{}' failed: {}", agent_name, e))
            })?;

        let reply = final_state
            .last_assistant_reply()
            .unwrap_or_else(|| "(no reply from sub-agent)".to_string());

        Ok(ToolCallContent { text: reply })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool() -> InvokeAgentTool {
        InvokeAgentTool::new(Arc::new(ReactBuildConfig::from_env()), Some(3))
    }

    #[test]
    fn spec_contains_required_fields() {
        let tool = make_tool();
        let spec = tool.spec();
        assert_eq!(spec.name, TOOL_INVOKE_AGENT);
        assert!(spec.description.is_some());
        let schema = &spec.input_schema;
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "agent"));
        assert!(required.iter().any(|v| v == "task"));
    }

    #[test]
    fn spec_lists_available_agents() {
        let tool = make_tool();
        let spec = tool.spec();
        let desc = spec.description.unwrap();
        assert!(desc.contains("dev"), "should list dev agent: {}", desc);
    }

    #[tokio::test]
    async fn depth_exceeded_returns_error() {
        let tool = InvokeAgentTool::new(Arc::new(ReactBuildConfig::from_env()), Some(2));
        let args = serde_json::json!({"agent": "dev", "task": "hello"});
        let ctx = ToolCallContext {
            depth: 2,
            ..Default::default()
        };
        let result = tool.call(args, Some(&ctx)).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("max sub-agent depth"), "error: {}", err);
    }

    #[tokio::test]
    async fn missing_agent_arg_returns_error() {
        let tool = make_tool();
        let args = serde_json::json!({"task": "hello"});
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("agent"));
    }

    #[tokio::test]
    async fn missing_task_arg_returns_error() {
        let tool = make_tool();
        let args = serde_json::json!({"agent": "dev"});
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("task"));
    }

    #[tokio::test]
    async fn unknown_agent_returns_error() {
        let tool = make_tool();
        let args = serde_json::json!({"agent": "nonexistent-xyz", "task": "hello"});
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nonexistent-xyz"));
    }
}
