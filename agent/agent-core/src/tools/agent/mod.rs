//! AgentTool: invoke a sub-agent by profile name.
//!
//! ```json
//! {"agent": "dev", "task": "Implement login API", "background": false}
//! ```
//! With `background: true`, starts in background and returns an `agent_id`:
//! ```json
//! {"agent": "dev", "task": "...", "background": true}
//! → {"agent_id": "sub-root-dev-0-3", "status": "running"}
//! ```
//! Use the `agent_get` tool to retrieve results from background agents.

pub mod build_config;
pub mod registry;
mod runner;
mod worktree;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::profile::list_available_profiles;
use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec, Tool};
use crate::agent::ReactBuildConfig;
use crate::tool_output_normalizer::{ToolOutputHint, ToolOutputStrategy};

pub use tool_core::tool_name::TOOL_AGENT;

pub(super) const DEFAULT_MAX_DEPTH: u32 = 3;

/// Global counter for unique agent_id generation.
static ASYNC_SEQ: AtomicU64 = AtomicU64::new(0);

pub struct AgentTool {
    pub(super) base_config: Arc<ReactBuildConfig>,
    pub(super) max_depth: u32,
    pub(super) registry: registry::AsyncAgentRegistry,
}

impl AgentTool {
    pub fn new(
        base_config: Arc<ReactBuildConfig>,
        max_depth: Option<u32>,
        registry: registry::AsyncAgentRegistry,
    ) -> Self {
        Self {
            base_config,
            max_depth: max_depth.unwrap_or(DEFAULT_MAX_DEPTH),
            registry,
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

    /// Generate a unique agent_id and extract common fields from args/ctx.
    fn prepare_agent_id(&self, agent_name: &str, depth: u32) -> String {
        let parent_thread_id = self
            .base_config
            .thread_id
            .as_deref()
            .unwrap_or("root");
        let seq = ASYNC_SEQ.fetch_add(1, Ordering::Relaxed);
        format!("sub-{}-{}-{}-{}", parent_thread_id, agent_name, depth, seq)
    }

    fn thread_id(&self) -> String {
        self.base_config
            .thread_id
            .clone()
            .unwrap_or_else(|| "root".to_string())
    }

    /// Spawn the agent execution as a tokio task and register it.
    /// Returns the JoinHandle and agent_id.
    fn spawn_agent_task(
        &self,
        agent_name: &str,
        args: Value,
        ctx: Option<&ToolCallContext>,
        agent_id: String,
    ) -> JoinHandle<Result<(ToolCallContent, registry::AgentCompletionStats), ToolSourceError>> {
        let base_config = self.base_config.clone();
        let ctx_clone = ctx.cloned();
        let registry = self.registry.clone();
        let agent_id_for_task = agent_id.clone();
        let agent_name_owned = agent_name.to_string();
        let agent_name_for_register = agent_name_owned.clone();

        let handle = tokio::spawn(async move {
            tracing::info!(
                agent = %agent_name_owned,
                agent_id = %agent_id_for_task,
                "Starting agent execution"
            );
            let result = worktree::invoke_single_agent(
                &base_config,
                args,
                ctx_clone.as_ref(),
            )
            .await;

            // Update registry with result.
            match &result {
                Ok((content, stats)) => {
                    let text = content.as_text().unwrap_or("").to_string();
                    registry.complete(&agent_id_for_task, text, stats.clone());
                }
                Err(e) => {
                    let stats = registry::AgentCompletionStats::default();
                    registry.fail(&agent_id_for_task, e.to_string(), stats);
                }
            }
            result
        });

        // Register with the AbortHandle for real cancellation.
        let abort_handle = handle.abort_handle();
        self.registry.register(
            agent_id.clone(),
            agent_name_for_register,
            self.thread_id(),
            Some(abort_handle),
        );

        handle
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        TOOL_AGENT
    }

    fn spec(&self) -> ToolSpec {
        let agents_desc = self.available_agents_description();
        let description = format!(
            "Delegate work to a sub-agent by profile name. Each sub-agent runs a full ReAct loop \
             with its own tools and system prompt, then returns a final reply.\n\
             \n\
             Required: `task` (full context; sub-agents have no memory of the current conversation).\n\
             Optional: `agent` (profile name) — if omitted, uses the built-in `default` profile \
             for simple, focused tasks. Default: \"default\".\n\
             \n\
             Optional: `background` (bool) — if true, starts agent in background and returns \
             `agent_id` immediately. Use `agent_get` tool to retrieve results. Default: false.\n\
             \n\
             Optional: `timeout` (number) — timeout in seconds. If sync call exceeds this time, \
             the agent transitions to background execution and returns `agent_id`. Default: 600.{}",
            agents_desc,
        );
        ToolSpec {
            name: TOOL_AGENT.to_string(),
            description: Some(description),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent": {
                        "type": "string",
                        "description": "Agent profile name or path to profile directory. If omitted, uses the built-in 'default' profile."
                    },
                    "task": {
                        "type": "string",
                        "description": "Natural-language task to delegate; include full context."
                    },
                    "background": {
                        "type": "boolean",
                        "description": "If true, start the agent in the background and return immediately with an agent_id. Use agent_get to retrieve the result later. Default: false.",
                        "default": false
                    },
                    "timeout": {
                        "type": "number",
                        "description": "Timeout in seconds. If a synchronous call exceeds this time, the agent transitions to background execution and returns an agent_id. Default: 600.",
                        "default": 600
                    },
                    "working_folder": {
                        "type": "string",
                        "description": "Override the working folder for this sub-agent."
                    },
                    "model_tier": {
                        "type": "string",
                        "enum": model_spec_core::ModelTier::variants().to_vec(),
                        "description": "Override the agent's model tier for this invocation."
                    },
                    "isolation": {
                        "type": "string",
                        "description": "worktree: create an isolated git worktree. none: explicitly disable worktree even if profile configures it. If omitted, uses profile default.",
                        "enum": ["worktree", "none"]
                    }
                },
                "required": ["task"]
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::SummaryOnly)),
        }
    }

    async fn call(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let agent_name = args
            .get("agent")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string();
        let task = args
            .get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolSourceError::InvalidInput("missing required argument: task".into())
            })?;
        let background = args
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let current_depth = ctx.map(|c| c.depth).unwrap_or(0);

        tracing::info!(
            agent = %agent_name,
            task_length = task.len(),
            background = background,
            depth = current_depth,
            "agent invoke"
        );

        if current_depth >= self.max_depth {
            tracing::warn!(
                agent = %agent_name,
                current_depth = current_depth,
                max_depth = self.max_depth,
                "Max depth reached, refusing agent invocation"
            );
            return Err(ToolSourceError::InvalidInput(format!(
                "max sub-agent depth ({}) reached; cannot invoke further agents",
                self.max_depth,
            )));
        }

        let agent_id = self.prepare_agent_id(&agent_name, current_depth);

        if background {
            // Spawn and return immediately.
            let agent_id_clone = agent_id.clone();
            self.spawn_agent_task(&agent_name, args, ctx, agent_id);
            let response = serde_json::json!({
                "agent_id": agent_id_clone,
                "status": "running"
            });
            Ok(ToolCallContent::text(response.to_string()))
        } else {
            // Spawn + wait with timeout.
            let timeout_sec = args
                .get("timeout")
                .and_then(|v| v.as_u64())
                .unwrap_or(600);
            let handle = self.spawn_agent_task(&agent_name, args, ctx, agent_id.clone());

            match tokio::time::timeout(
                std::time::Duration::from_secs(timeout_sec),
                handle,
            )
            .await
            {
                Ok(Ok(Ok((content, _)))) => Ok(content),
                Ok(Ok(Err(e))) => Err(e),
                Ok(Err(_)) => {
                    // Task panicked.
                    self.registry.fail(
                        &agent_id,
                        "task panicked".into(),
                        registry::AgentCompletionStats::default(),
                    );
                    Err(ToolSourceError::Transport("sub-agent task panicked".into()))
                }
                Err(_) => {
                    // Timeout — transition to background.
                    // The task continues running; we just return the agent_id.
                    self.registry.mark_background(&agent_id);
                    let response = serde_json::json!({
                        "agent_id": agent_id,
                        "status": "background",
                        "message": format!("timeout after {}s, continuing in background", timeout_sec)
                    });
                    Ok(ToolCallContent::text(response.to_string()))
                }
            }
        }
    }
}
