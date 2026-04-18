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

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::cli_run::{build_config_from_profile, list_available_profiles, resolve_profile};
use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;
use crate::{build_react_runner, resolve_tier_and_build_config, ReactBuildConfig, ToolOutputHint, ToolOutputStrategy};

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
                                    "enum": model_spec_core::spec::ModelTier::variants().to_vec(),
                                    "description": "Optional: override the agent's model tier for this invocation. Switches to the best model of this tier from the same provider."
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

impl InvokeAgentTool {
    async fn call_single(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let current_depth = ctx.map(|c| c.depth).unwrap_or(0);
        let agent_name = args
            .get("agent")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        
        tracing::debug!(
            agent = %agent_name,
            current_depth = current_depth,
            max_depth = self.max_depth,
            "Starting single agent invocation"
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

        tracing::debug!(agent = %agent_name, "Proceeding with single agent execution");
        self.call_single_exec(args, ctx).await
    }

    /// Core execution logic for a single agent invocation.
    async fn call_single_exec(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {

        let agent_name = args.get("agent").and_then(|v| v.as_str()).ok_or_else(|| {
            ToolSourceError::InvalidInput("missing required argument: agent".into())
        })?;

        let task = args.get("task").and_then(|v| v.as_str()).ok_or_else(|| {
            ToolSourceError::InvalidInput("missing required argument: task".into())
        })?;

        tracing::info!(
            agent = %agent_name,
            task_length = task.len(),
            depth = ctx.map(|c| c.depth).unwrap_or(0),
            "Starting execution of agent task"
        );

        let working_folder_override = args
            .get("working_folder")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from);

        if let Some(ref folder) = working_folder_override {
            tracing::debug!(
                agent = %agent_name,
                working_folder = %folder.display(),
                "Using custom working folder"
            );
        }

        tracing::debug!(agent = %agent_name, "Resolving agent profile");
        let profile = resolve_profile(agent_name).map_err(|e| {
            tracing::error!(agent = %agent_name, error = %e, "Failed to resolve agent profile");
            ToolSourceError::InvalidInput(format!(
                "failed to resolve agent '{}': {}",
                agent_name, e
            ))
        })?;

        tracing::debug!(
            agent = %agent_name,
            profile_name = %profile.name,
            "Building sub-agent configuration"
        );
        let mut sub_config = build_config_from_profile(
            &profile,
            &self.base_config,
            working_folder_override.as_deref(),
        );

        tracing::debug!(
            agent = %agent_name,
            profile_name = %profile.name,
            profile_tier = ?sub_config.model_tier,
            parent_tier = ?self.base_config.model_tier,
            profile_model = ?sub_config.model,
            parent_model = ?self.base_config.model,
            profile_llm_provider = ?sub_config.llm_provider,
            parent_llm_provider = ?self.base_config.llm_provider,
            "Built sub-agent config from profile with model details"
        );

        if let Some(tier_str) = args.get("model_tier").and_then(|v| v.as_str()) {
            tracing::info!(
                agent = %agent_name,
                tier_override = %tier_str,
                current_profile_tier = ?sub_config.model_tier,
                current_profile_model = ?sub_config.model,
                "Processing model_tier override request"
            );
            
            match serde_json::from_str::<crate::model_spec::ModelTier>(tier_str) {
                Ok(tier) => {
                    tracing::info!(
                        agent = %agent_name,
                        old_tier = ?sub_config.model_tier,
                        new_tier = ?tier,
                        old_model = ?sub_config.model,
                        "Overriding model_tier from invoke_agent arguments"
                    );
                    sub_config.model_tier = Some(tier);
                }
                Err(e) => {
                    tracing::warn!(
                        agent = %agent_name,
                        tier = %tier_str, 
                        error = %e, 
                        "Invalid model_tier format, ignoring override"
                    );
                }
            }
        }

        // Log final model configuration before tier resolution
        tracing::debug!(
            agent = %agent_name,
            final_tier_before_resolution = ?sub_config.model_tier,
            final_model_before_resolution = ?sub_config.model,
            final_provider_before_resolution = ?sub_config.llm_provider,
            "Final model configuration before tier resolution"
        );

        // Sub-agent gets its own unique thread_id (checkpointer key) so its
        // graph state is isolated from the parent.
        // trace_thread_id is inherited unchanged so all LLM calls across the
        // hierarchy share the same X-Thread-Id for external tracing.
        let depth = ctx.map_or(0, |c| c.depth);
        let parent_thread_id = self.base_config.thread_id.as_deref().unwrap_or("root");
        let sub_thread_id = format!("sub-{}-{}-{}", parent_thread_id, agent_name, depth);
        sub_config.thread_id = Some(sub_thread_id.clone());
        sub_config.trace_thread_id = self.base_config.trace_thread_id.clone();

        tracing::debug!(
            agent = %agent_name,
            thread_id = %sub_thread_id,
            depth = depth,
            "Configured sub-agent thread isolation"
        );

        tracing::debug!(
            agent = %agent_name,
            tier_to_resolve = ?sub_config.model_tier,
            current_model = ?sub_config.model,
            "Resolving tier and building final config"
        );
        let sub_config = resolve_tier_and_build_config(&sub_config).await;
        
        tracing::info!(
            agent = %agent_name,
            resolved_model = ?sub_config.model,
            resolved_provider = ?sub_config.llm_provider,
            resolved_base_url = ?sub_config.openai_base_url,
            tier_resolution_complete = true,
            "Model tier resolved successfully"
        );

        tracing::debug!(agent = %agent_name, "Building React runner");
        let runner = build_react_runner(&sub_config, None, false)
            .await
            .map_err(|e| {
                tracing::error!(agent = %agent_name, error = %e, "Failed to build sub-agent runner");
                ToolSourceError::Transport(format!(
                    "failed to build sub-agent '{}': {}",
                    agent_name, e
                ))
            })?;

        tracing::debug!(agent = %agent_name, "Starting sub-agent execution");
        let on_event = ctx.and_then(|c| c.any_stream_event_sender.clone()).map(|sender| {
            move |event: crate::stream::StreamEvent<crate::state::ReActState>| {
                sender(crate::cli_run::AnyStreamEvent::React(event));
            }
        });
        let any_sender = ctx.and_then(|c| c.any_stream_event_sender.clone());

        let outcome = runner
            .stream_with_config(task, None, on_event, any_sender)
            .await
            .map_err(|e| {
                tracing::error!(agent = %agent_name, error = %e, "Sub-agent execution failed");
                ToolSourceError::Transport(format!("sub-agent '{}' failed: {}", agent_name, e))
            })?;

        let reply = match outcome {
            crate::runner_common::StreamRunOutcome::Finished(final_state) => {
                let reply = final_state
                    .last_assistant_reply()
                    .unwrap_or_else(|| "(no reply from sub-agent)".to_string());
                tracing::info!(
                    agent = %agent_name,
                    reply_length = reply.len(),
                    "Sub-agent completed successfully"
                );
                reply
            }
            crate::runner_common::StreamRunOutcome::Cancelled => {
                tracing::warn!(agent = %agent_name, "Sub-agent was cancelled");
                "(sub-agent cancelled)".to_string()
            }
        };

        Ok(ToolCallContent::text(reply))
    }

    /// Invoke multiple agents concurrently
    async fn call_multiple(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let agents = args
            .get("agents")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ToolSourceError::InvalidInput("agents must be an array".into()))?;

        if agents.is_empty() {
            return Err(ToolSourceError::InvalidInput(
                "agents array cannot be empty".into(),
            ));
        }

        let fail_fast = args
            .get("fail_fast")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        tracing::info!(
            agent_count = agents.len(),
            fail_fast = fail_fast,
            depth = ctx.map(|c| c.depth).unwrap_or(0),
            "Starting concurrent execution of {} agents",
            agents.len()
        );

        // Validate all agent specs before spawning tasks
        let mut agent_names = Vec::new();
        for (idx, agent_spec) in agents.iter().enumerate() {
            let agent_name = agent_spec
                .get("agent")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            
            if agent_spec.get("agent").and_then(|v| v.as_str()).is_none() {
                tracing::error!(index = idx, "Agent spec missing required field: agent");
                return Err(ToolSourceError::InvalidInput(format!(
                    "agent spec at index {} missing required field: agent",
                    idx
                )));
            }
            if agent_spec.get("task").and_then(|v| v.as_str()).is_none() {
                tracing::error!(agent = %agent_name, index = idx, "Agent spec missing required field: task");
                return Err(ToolSourceError::InvalidInput(format!(
                    "agent spec at index {} missing required field: task",
                    idx
                )));
            }
            agent_names.push(agent_name.to_string());
        }

        tracing::debug!(
            agents = ?agent_names,
            "Validated all agent specifications"
        );

        let mut handles = vec![];
        for (idx, agent_spec) in agents.iter().enumerate() {
            let agent_name = agent_spec
                .get("agent")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            
            tracing::debug!(
                agent = %agent_name,
                index = idx,
                "Spawning concurrent agent task"
            );
            
            let args = agent_spec.clone();
            let ctx = ctx.cloned();
            let base_config = self.base_config.clone();
            let max_depth = self.max_depth;

            let handle = tokio::spawn(async move {
                invoke_single_agent(&base_config, args, ctx.as_ref(), max_depth).await
            });

            handles.push(handle);
        }

        tracing::debug!("Waiting for all concurrent agent tasks to complete");
        let results = futures::future::join_all(handles).await;

        // Aggregate results
        let mut successful = vec![];
        let mut failed = vec![];

        for (idx, result) in results.into_iter().enumerate() {
            let unknown_agent = "unknown".to_string();
            let agent_name = agent_names.get(idx).unwrap_or(&unknown_agent);
            
            match result {
                Ok(Ok(content)) => {
                    let text = content.as_text().unwrap().to_string();
                    tracing::info!(
                        agent = %agent_name,
                        index = idx,
                        reply_length = text.len(),
                        "Agent completed successfully"
                    );
                    successful.push((idx, text));
                }
                Ok(Err(e)) => {
                    tracing::error!(
                        agent = %agent_name,
                        index = idx,
                        error = %e,
                        "Agent failed during execution"
                    );
                    if fail_fast {
                        tracing::warn!("Fail-fast mode enabled, stopping execution");
                        return Err(ToolSourceError::Transport(format!(
                            "agent {} failed (fail-fast mode): {}",
                            idx, e
                        )));
                    }
                    failed.push((idx, e.to_string()));
                }
                Err(e) => {
                    tracing::error!(
                        agent = %agent_name,
                        index = idx,
                        error = %e,
                        "Agent task panicked"
                    );
                    if fail_fast {
                        tracing::warn!("Fail-fast mode enabled, stopping execution");
                        return Err(ToolSourceError::Transport(format!(
                            "agent {} panicked (fail-fast mode): {}",
                            idx, e
                        )));
                    }
                    failed.push((idx, format!("panic: {}", e)));
                }
            }
        }

        tracing::info!(
            successful_count = successful.len(),
            failed_count = failed.len(),
            total_count = agent_names.len(),
            "Concurrent agent execution completed"
        );

        // Format aggregated result
        let mut output = String::new();
        output.push_str(&format!(
            "Concurrent agent execution completed: {} succeeded, {} failed\n\n",
            successful.len(),
            failed.len()
        ));

        if !successful.is_empty() {
            output.push_str("## Successful Results:\n");
            for (idx, text) in successful {
                output.push_str(&format!("\n### Agent {}:\n{}\n", idx, text));
            }
        }

        if !failed.is_empty() {
            output.push_str("\n## Failed Agents:\n");
            for (idx, error) in failed {
                output.push_str(&format!("- Agent {}: {}\n", idx, error));
            }
        }

        Ok(ToolCallContent::text(output))
    }

    /// Invoke multiple agents asynchronously (fire-and-forget)
    async fn call_multiple_async(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let agents = args
            .get("agents")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ToolSourceError::InvalidInput("agents must be an array".into()))?;

        if agents.is_empty() {
            return Err(ToolSourceError::InvalidInput(
                "agents array cannot be empty".into(),
            ));
        }

        tracing::info!(
            agent_count = agents.len(),
            depth = ctx.map(|c| c.depth).unwrap_or(0),
            "Starting async execution of {} agents",
            agents.len()
        );

        // Validate all agent specs before spawning tasks
        let mut agent_names = vec![];
        for (idx, agent_spec) in agents.iter().enumerate() {
            let agent_name = agent_spec
                .get("agent")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            
            if agent_spec.get("agent").and_then(|v| v.as_str()).is_none() {
                tracing::error!(index = idx, "Agent spec missing required field: agent");
                return Err(ToolSourceError::InvalidInput(format!(
                    "agent spec at index {} missing required field: agent",
                    idx
                )));
            }
            if agent_spec.get("task").and_then(|v| v.as_str()).is_none() {
                tracing::error!(agent = %agent_name, index = idx, "Agent spec missing required field: task");
                return Err(ToolSourceError::InvalidInput(format!(
                    "agent spec at index {} missing required field: task",
                    idx
                )));
            }
            if let Some(name) = agent_spec.get("agent").and_then(|v| v.as_str()) {
                tracing::debug!(agent = %name, index = idx, "Validating agent profile");
                
                // Validate agent exists
                resolve_profile(name).map_err(|e| {
                    tracing::error!(agent = %name, index = idx, error = %e, "Failed to resolve agent profile");
                    ToolSourceError::InvalidInput(format!(
                        "failed to resolve agent '{}' at index {}: {}",
                        name, idx, e
                    ))
                })?;
                agent_names.push(name.to_string());
            }
        }

        tracing::debug!(
            agents = ?agent_names,
            "All agent profiles validated successfully"
        );

        // Spawn all agents in background
        let base_config = self.base_config.clone();
        let max_depth = self.max_depth;
        let ctx_clone = ctx.cloned();

        tracing::debug!("Spawning {} agents in background", agent_names.len());
        
        for (idx, agent_spec) in agents.iter().enumerate() {
            let agent_name = agent_spec
                .get("agent")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            
            tracing::debug!(
                agent = %agent_name,
                index = idx,
                "Spawning async agent task"
            );
            
            let args = agent_spec.clone();
            let ctx = ctx_clone.clone();
            let base_config = base_config.clone();

            tokio::spawn(async move {
                tracing::info!(
                    agent = %agent_name,
                    index = idx,
                    "Starting async agent execution"
                );
                
                if let Err(e) =
                    invoke_single_agent(&base_config, args, ctx.as_ref(), max_depth).await
                {
                    tracing::error!(
                        agent = %agent_name,
                        index = idx,
                        error = %e,
                        "Async agent invocation failed"
                    );
                } else {
                    tracing::info!(
                        agent = %agent_name,
                        index = idx,
                        "Async agent execution completed"
                    );
                }
            });
        }

        tracing::info!(
            agent_count = agent_names.len(),
            agents = ?agent_names,
            "All async agent tasks spawned successfully"
        );

        Ok(ToolCallContent::text(format!(
            "Started {} agent(s) in background: {}",
            agent_names.len(),
            agent_names.join(", ")
        )))
    }
}

/// Helper function to invoke a single agent (used by batch calls)
async fn invoke_single_agent(
    base_config: &Arc<ReactBuildConfig>,
    args: Value,
    ctx: Option<&ToolCallContext>,
    max_depth: u32,
) -> Result<ToolCallContent, ToolSourceError> {
    let current_depth = ctx.map(|c| c.depth).unwrap_or(0);
    let agent_name = args
        .get("agent")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    tracing::debug!(
        agent = %agent_name,
        current_depth = current_depth,
        max_depth = max_depth,
        "invoke_single_agent called"
    );

    if current_depth >= max_depth {
        tracing::warn!(
            agent = %agent_name,
            current_depth = current_depth,
            max_depth = max_depth,
            "Max depth reached in invoke_single_agent"
        );
        return Err(ToolSourceError::InvalidInput(format!(
            "max sub-agent depth ({}) reached; cannot invoke further agents",
            max_depth,
        )));
    }

    let task = args
        .get("task")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolSourceError::InvalidInput("missing required argument: task".into()))?;

    tracing::info!(
        agent = %agent_name,
        task_length = task.len(),
        depth = current_depth,
        "Starting single agent invocation"
    );

    let working_folder_override = args
        .get("working_folder")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);

    if let Some(ref folder) = working_folder_override {
        tracing::debug!(
            agent = %agent_name,
            working_folder = %folder.display(),
            "Using custom working folder"
        );
    }

    tracing::debug!(agent = %agent_name, "Resolving agent profile");
    let profile = resolve_profile(agent_name).map_err(|e| {
        tracing::error!(agent = %agent_name, error = %e, "Failed to resolve agent profile");
        ToolSourceError::InvalidInput(format!("failed to resolve agent '{}': {}", agent_name, e))
    })?;

    tracing::debug!(
        agent = %agent_name,
        profile_name = %profile.name,
        "Building sub-agent configuration"
    );
    let mut sub_config =
        build_config_from_profile(&profile, base_config, working_folder_override.as_deref());

    tracing::debug!(
        agent = %agent_name,
        profile_name = %profile.name,
        profile_tier = ?sub_config.model_tier,
        parent_tier = ?base_config.model_tier,
        profile_model = ?sub_config.model,
        parent_model = ?base_config.model,
        profile_llm_provider = ?sub_config.llm_provider,
        parent_llm_provider = ?base_config.llm_provider,
        "Built sub-agent config from profile with model details"
    );

    if let Some(tier_str) = args.get("model_tier").and_then(|v| v.as_str()) {
        tracing::info!(
            agent = %agent_name,
            tier_override = %tier_str,
            current_profile_tier = ?sub_config.model_tier,
            current_profile_model = ?sub_config.model,
            "Processing model_tier override request"
        );
        
        match serde_json::from_str::<crate::model_spec::ModelTier>(tier_str) {
            Ok(tier) => {
                tracing::info!(
                    agent = %agent_name,
                    old_tier = ?sub_config.model_tier,
                    new_tier = ?tier,
                    old_model = ?sub_config.model,
                    "Overriding model_tier from invoke_agent arguments"
                );
                sub_config.model_tier = Some(tier);
            }
            Err(e) => {
                tracing::warn!(
                    agent = %agent_name,
                    tier = %tier_str, 
                    error = %e, 
                    "Invalid model_tier format, ignoring override"
                );
            }
        }
    }

    // Log final model configuration before tier resolution
    tracing::debug!(
        agent = %agent_name,
        final_tier_before_resolution = ?sub_config.model_tier,
        final_model_before_resolution = ?sub_config.model,
        final_provider_before_resolution = ?sub_config.llm_provider,
        "Final model configuration before tier resolution"
    );

    // Sub-agent gets its own unique thread_id (checkpointer key) so its
    // graph state is isolated from the parent.
    // trace_thread_id is inherited unchanged so all LLM calls across the
    // hierarchy share the same X-Thread-Id for external tracing.
    let depth = ctx.map_or(0, |c| c.depth);
    let parent_thread_id = base_config.thread_id.as_deref().unwrap_or("root");
    let sub_thread_id = format!("sub-{}-{}-{}", parent_thread_id, agent_name, depth);
    sub_config.thread_id = Some(sub_thread_id.clone());
    sub_config.trace_thread_id = base_config.trace_thread_id.clone();

    tracing::debug!(
        agent = %agent_name,
        thread_id = %sub_thread_id,
        depth = depth,
        "Configured sub-agent thread isolation"
    );

    tracing::debug!(
        agent = %agent_name,
        tier_to_resolve = ?sub_config.model_tier,
        current_model = ?sub_config.model,
        "Resolving tier and building final config"
    );
    let sub_config = resolve_tier_and_build_config(&sub_config).await;
    
    tracing::info!(
        agent = %agent_name,
        resolved_model = ?sub_config.model,
        resolved_provider = ?sub_config.llm_provider,
        resolved_base_url = ?sub_config.openai_base_url,
        tier_resolution_complete = true,
        "Model tier resolved successfully"
    );

    tracing::debug!(agent = %agent_name, "Building React runner");
    let runner = build_react_runner(&sub_config, None, false)
        .await
        .map_err(|e| {
            tracing::error!(agent = %agent_name, error = %e, "Failed to build sub-agent runner");
            ToolSourceError::Transport(format!("failed to build sub-agent '{}': {}", agent_name, e))
        })?;

    tracing::debug!(agent = %agent_name, "Starting sub-agent execution");
    let on_event = ctx.and_then(|c| c.any_stream_event_sender.clone()).map(|sender| {
        move |event: crate::stream::StreamEvent<crate::state::ReActState>| {
            sender(crate::cli_run::AnyStreamEvent::React(event));
        }
    });
    let any_sender = ctx.and_then(|c| c.any_stream_event_sender.clone());

    let outcome = runner
        .stream_with_config(task, None, on_event, any_sender)
        .await
        .map_err(|e| {
            tracing::error!(agent = %agent_name, error = %e, "Sub-agent execution failed");
            ToolSourceError::Transport(format!("sub-agent '{}' failed: {}", agent_name, e))
        })?;

    let reply = match outcome {
        crate::runner_common::StreamRunOutcome::Finished(final_state) => {
            let reply = final_state
                .last_assistant_reply()
                .unwrap_or_else(|| "(no reply from sub-agent)".to_string());
            tracing::info!(
                agent = %agent_name,
                reply_length = reply.len(),
                "Sub-agent completed successfully"
            );
            reply
        }
        crate::runner_common::StreamRunOutcome::Cancelled => {
            tracing::warn!(agent = %agent_name, "Sub-agent was cancelled");
            "(sub-agent cancelled)".to_string()
        }
    };

    Ok(ToolCallContent::text(reply))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool() -> InvokeAgentTool {
        InvokeAgentTool::new(Arc::new(ReactBuildConfig::from_env()), Some(3))
    }

    #[tokio::test]
    async fn depth_exceeded_returns_error() {
        let tool = InvokeAgentTool::new(Arc::new(ReactBuildConfig::from_env()), Some(2));
        let args = serde_json::json!({
            "agents": [{"agent": "dev", "task": "hello"}]
        });
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
    async fn missing_agents_arg_returns_error() {
        let tool = make_tool();
        let args = serde_json::json!({"fail_fast": false});
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("agents"), "error: {}", msg);
    }

    #[tokio::test]
    async fn empty_agents_array_returns_error() {
        let tool = make_tool();
        let args = serde_json::json!({"agents": []});
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[tokio::test]
    async fn missing_task_in_single_item_returns_error() {
        let tool = make_tool();
        let args = serde_json::json!({"agents": [{"agent": "dev"}]});
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("task"));
    }

    #[tokio::test]
    async fn unknown_agent_returns_error() {
        let tool = make_tool();
        let args = serde_json::json!({
            "agents": [{"agent": "nonexistent-xyz", "task": "hello"}]
        });
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nonexistent-xyz"));
    }

    #[tokio::test]
    async fn batch_call_missing_agent_in_array_returns_error() {
        let tool = make_tool();
        let args = serde_json::json!({
            "agents": [
                {"task": "hello"}
            ]
        });
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("agent"));
    }

    #[tokio::test]
    async fn batch_call_missing_task_in_array_returns_error() {
        let tool = make_tool();
        let args = serde_json::json!({
            "agents": [
                {"agent": "dev"}
            ]
        });
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("task"));
    }

    #[tokio::test]
    async fn batch_call_with_invalid_agents_array_returns_error() {
        let tool = make_tool();
        let args = serde_json::json!({
            "agents": "not-an-array"
        });
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("agents") && msg.contains("array"),
            "error: {}",
            msg
        );
    }

    struct MockTierResolver {
        light_model_id: String,
    }

    #[async_trait::async_trait]
    impl crate::TierResolver for MockTierResolver {
        async fn resolve_tier(
            &self,
            _config: &ReactBuildConfig,
            tier: crate::model_spec::ModelTier,
        ) -> Option<crate::ResolvedTierModel> {
            assert_eq!(
                tier,
                crate::model_spec::ModelTier::Light,
                "explore agent should request Light tier"
            );
            Some(crate::ResolvedTierModel {
                model_id: self.light_model_id.clone(),
                base_url: Some("https://mock.test/v1".into()),
                api_key: Some("sk-mock".into()),
                provider_type: Some("openai_compat".into()),
            })
        }
    }

    #[tokio::test]
    async fn explore_agent_resolves_light_tier_model() {
        let profile = resolve_profile("explore").expect("explore profile should load");
        assert_eq!(
            profile.model.as_ref().and_then(|m| m.tier),
            Some(crate::model_spec::ModelTier::Light),
            "explore agent config.yaml should declare tier: light"
        );

        let mut parent_config = ReactBuildConfig::from_env();
        parent_config.model = None;
        parent_config.openai_base_url = None;
        parent_config.openai_api_key = None;
        parent_config.llm_provider = None;
        let sub_config = build_config_from_profile(&profile, &parent_config, None);
        assert_eq!(
            sub_config.model_tier,
            Some(crate::model_spec::ModelTier::Light),
            "build_config_from_profile should propagate explore's light tier"
        );

        let resolver = MockTierResolver {
            light_model_id: "anthropic/claude-haiku-4".to_string(),
        };
        let resolved =
            crate::resolve_tier_and_build_config_with_resolver(&sub_config, &resolver).await;

        assert_eq!(
            resolved.model.as_deref(),
            Some("anthropic/claude-haiku-4"),
            "resolved model should be the light-tier model from MockTierResolver"
        );
        assert!(
            resolved.model_tier.is_none(),
            "model_tier should be cleared after resolution"
        );
        assert_eq!(
            resolved.openai_base_url.as_deref(),
            Some("https://mock.test/v1"),
            "base_url should come from resolved tier model"
        );
        assert_eq!(
            resolved.openai_api_key.as_deref(),
            Some("sk-mock"),
            "api_key should come from resolved tier model"
        );
        assert_eq!(
            resolved.llm_provider.as_deref(),
            Some("openai_compat"),
            "provider_type should come from resolved tier model"
        );
    }

    #[tokio::test]
    async fn explore_agent_config_inheritance_with_parent_tier() {
        // Test that explore's tier: light overrides parent config
        let profile = resolve_profile("explore").expect("explore profile should load");
        
        let mut parent_config = ReactBuildConfig::from_env();
        parent_config.model_tier = Some(crate::model_spec::ModelTier::Strong); // Parent has Strong tier
        
        let sub_config = build_config_from_profile(&profile, &parent_config, None);
        
        // Explore's light tier should override parent's strong tier
        assert_eq!(
            sub_config.model_tier,
            Some(crate::model_spec::ModelTier::Light),
            "explore's tier: light should override parent's tier"
        );
    }
}
