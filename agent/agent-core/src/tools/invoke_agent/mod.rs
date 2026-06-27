//! InvokeAgentTool: dynamically invoke a sub-agent by profile name at runtime.
//!
//! Unlike `AgentTool` (which wraps a pre-built `ReactRunner`), this tool resolves
//! an agent profile by name, builds a fresh `ReactRunner`, and runs it — all at
//! call time. This lets the LLM decide which sub-agent to delegate to.
//!
//! ## Usage
//!
//! Input is always a non-empty **`agents`** array. Use one element for a single sub-agent.
//!
//! ### Concurrent invocation (default)
//! ```json
//! {
//!   "agents": [
//!     {"agent": "dev", "task": "Implement login API"},
//!     {"agent": "explore", "task": "Analyze code structure"}
//!   ],
//!   "fail_fast": false
//! }
//! ```
//!
//! ### Async (fire-and-forget)
//! ```json
//! {
//!   "agents": [
//!     {"agent": "dev", "task": "Run background analysis"}
//!   ],
//!   "async": true
//! }
//! ```
//! When `async: true`, each agent starts in the background and the call returns immediately
//! without waiting for results.
//!
//! ## Error Handling
//!
//! - **One or more agents, sync**: Errors are returned immediately (or aggregated when `fail_fast` is false and multiple agents ran).
//! - **Multiple agents, fail_fast=true**: Stops on first error; other runs may still be in flight.
//! - **Multiple agents, fail_fast=false**: Collects all errors and returns an aggregated result.
//! - **Async mode**: Errors are logged but not returned (fire-and-forget behavior).

mod dispatch;
mod runner;
mod worktree;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use loom_react_config::profile::list_available_profiles;
use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec, Tool};
use loom_react_config::ReactBuildConfig;
use loom_types::tool_output_normalizer::{ToolOutputHint, ToolOutputStrategy};

pub use loom_types::tools::tool_name::TOOL_INVOKE_AGENT;
pub(super) const DEFAULT_MAX_DEPTH: u32 = 3;

pub struct InvokeAgentTool {
    pub(super) base_config: Arc<ReactBuildConfig>,
    pub(super) max_depth: u32,
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
            "Delegate work to one or more sub-agents by profile name. Each sub-agent runs a full \
             ReAct loop with its own tools and system prompt, then returns a final reply.\n\
             \n\
             Always pass a non-empty `agents` array. For a single delegation use one element: \
             `{{ \"agents\": [{{ \"agent\": \"...\", \"task\": \"...\" }}] }}`. \
             Provide full context in each `task`; sub-agents have no memory of the current conversation.{}",
            agents_desc,
        );
        ToolSpec {
            name: TOOL_INVOKE_AGENT.to_string(),
            description: Some(description),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agents": {
                        "type": "array",
                        "minItems": 1,
                        "description": "Non-empty list of delegations. Each item has 'agent', 'task', and optional 'working_folder'.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "agent": {
                                    "type": "string",
                                    "description": "Agent profile name or path to profile directory."
                                },
                                "task": {
                                    "type": "string",
                                    "description": "Natural-language task to delegate; include full context."
                                },
                                "working_folder": {
                                    "type": "string",
                                    "description": "Optional: override working folder for this sub-agent."
                                },
                                "model_tier": {
                                    "type": "string",
                                    "enum": model_spec_core::ModelTier::variants().to_vec(),
                                    "description": "Optional: override the agent's model tier for this invocation. Switches to the best model of this tier from the same provider."
                                },
                                "isolation": {
                                    "type": "string",
                                    "description": "Optional: create an isolated git worktree for this sub-agent. Values: 'worktree'. If omitted, uses shared working directory.",
                                    "enum": ["worktree"]
                                },
                                "estimated_paths": {
                                    "type": "array",
                                    "description": "Optional: list of file/dir paths the task is expected to modify. Used for pre-merge conflict detection when multiple agents run in parallel.",
                                    "items": { "type": "string" }
                                }
                            },
                            "required": ["agent", "task"]
                        }
                    },
                    "fail_fast": {
                        "type": "boolean",
                        "description": "When multiple agents run in parallel: if true, stop on first error. If false (default), continue and collect all results. Ignored when only one agent or when async is true.",
                        "default": false
                    },
                    "async": {
                        "type": "boolean",
                        "description": "If true, start all listed agent(s) in the background and return immediately without waiting for results. Default: false.",
                        "default": false
                    }
                },
                "required": ["agents"]
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::SummaryOnly)),
        }
    }

    async fn call(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let is_async = args.get("async").and_then(|v| v.as_bool()).unwrap_or(false);

        let agents = args
            .get("agents")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ToolSourceError::InvalidInput(
                    "missing or invalid required argument: agents (must be a non-empty array)"
                        .into(),
                )
            })?;
        if agents.is_empty() {
            return Err(ToolSourceError::InvalidInput(
                "agents array cannot be empty".into(),
            ));
        }

        tracing::info!(
            agent_count = agents.len(),
            is_async = is_async,
            depth = ctx.map(|c| c.depth).unwrap_or(0),
            "invoke_agent called with {} agents",
            agents.len()
        );

        if is_async {
            tracing::debug!("Starting async invocation of {} agents", agents.len());
            return self.call_multiple_async(args, ctx).await;
        }

        if agents.len() == 1 {
            let agent_name = agents[0]
                .get("agent")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            tracing::debug!("Starting single agent invocation: {}", agent_name);
            return self.call_single(agents[0].clone(), ctx).await;
        }

        tracing::debug!("Starting concurrent invocation of {} agents", agents.len());
        self.call_multiple(args, ctx).await
    }
}
