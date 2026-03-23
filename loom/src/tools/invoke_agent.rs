//! InvokeAgentTool: dynamically invoke a sub-agent by profile name at runtime.
//!
//! Unlike `AgentTool` (which wraps a pre-built `ReactRunner`), this tool resolves
//! an agent profile by name, builds a fresh `ReactRunner`, and runs it — all at
//! call time. This lets the LLM decide which sub-agent to delegate to.
//!
//! # Concurrency Control
//!
//! This tool supports both single and batch concurrent agent invocation with
//! **global concurrency control** to prevent resource exhaustion.
//!
//! ## Global Semaphore
//!
//! All `invoke_agent` calls (single, batch, or nested) share a global semaphore
//! that limits the maximum number of concurrent agent executions:
//!
//! - **Default limit**: 3 concurrent agents
//! - **Configuration**: Set `INVOKE_AGENT_MAX_CONCURRENT` environment variable
//! - **Behavior**: When the limit is reached, new invocations wait until a slot is available
//!
//! ## Usage Modes
//!
//! ### Single Invocation (Backward Compatible)
//! ```json
//! {
//!   "agent": "dev",
//!   "task": "Implement user authentication"
//! }
//! ```
//!
//! ### Batch Concurrent Invocation
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
//! ### Async Invocation (Fire-and-Forget)
//! ```json
//! {
//!   "agent": "dev",
//!   "task": "Run background analysis",
//!   "async": true
//! }
//! ```
//! When `async: true`, the agent starts in the background and the call returns immediately
//! without waiting for results. Useful for long-running tasks that don't need immediate feedback.
//!
//! ## Error Handling
//!
//! - **Single mode**: Errors are returned immediately
//! - **Batch mode with fail_fast=true**: Stops on first error, other running agents continue
//! - **Batch mode with fail_fast=false**: Collects all errors and returns aggregated result
//! - **Async mode**: Errors are logged but not returned (fire-and-forget behavior)

use std::sync::Arc;

use async_trait::async_trait;
use once_cell::sync::Lazy;
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::cli_run::{build_config_from_profile, list_available_profiles, resolve_profile};
use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;
use crate::{build_react_runner, ReactBuildConfig, ToolOutputHint, ToolOutputStrategy};

pub const TOOL_INVOKE_AGENT: &str = "invoke_agent";
const DEFAULT_MAX_DEPTH: u32 = 3;
const DEFAULT_MAX_CONCURRENT: usize = 3;

/// Global concurrency limiter for all invoke_agent calls.
///
/// This semaphore ensures that the total number of concurrently running agents
/// (including nested calls) never exceeds the configured limit, preventing
/// resource exhaustion and API rate limit issues.
///
/// # Configuration
///
/// Set via `INVOKE_AGENT_MAX_CONCURRENT` environment variable (default: 3).
///
/// # Thread Safety
///
/// - Initialized once at process startup using `Lazy`
/// - Shared across all threads using `Arc`
/// - Thread-safe access via `tokio::sync::Semaphore`
static INVOKE_AGENT_SEMAPHORE: Lazy<Arc<Semaphore>> = Lazy::new(|| {
    let max_concurrent = std::env::var("INVOKE_AGENT_MAX_CONCURRENT")
        .ok()
        .and_then(|s| s.parse().ok())
        .and_then(|n| if n > 0 && n <= 100 { Some(n) } else { None })
        .unwrap_or(DEFAULT_MAX_CONCURRENT);
    
    Arc::new(Semaphore::new(max_concurrent))
});

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
                        "description": "Agent profile name (e.g. 'dev', 'agent-builder') or path to profile directory. Use this for single agent invocation."
                    },
                    "task": {
                        "type": "string",
                        "description": "Natural-language task to delegate. Include full context; the sub-agent has no memory of the current conversation."
                    },
                    "working_folder": {
                        "type": "string",
                        "description": "Optional: override working folder for the sub-agent."
                    },
                    "agents": {
                        "type": "array",
                        "description": "Array of agent invocations for batch concurrent execution. Each item should have 'agent', 'task', and optional 'working_folder'.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "agent": {
                                    "type": "string",
                                    "description": "Agent profile name"
                                },
                                "task": {
                                    "type": "string",
                                    "description": "Task to delegate to this agent"
                                },
                                "working_folder": {
                                    "type": "string",
                                    "description": "Optional: override working folder"
                                }
                            },
                            "required": ["agent", "task"]
                        }
                    },
                    "fail_fast": {
                        "type": "boolean",
                        "description": "If true, stop on first error. If false (default), continue and collect all results.",
                        "default": false
                    },
                    "async": {
                        "type": "boolean",
                        "description": "If true, start agent(s) in background and return immediately without waiting for results. Useful for fire-and-forget tasks. Default: false.",
                        "default": false
                    }
                }
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
        let is_async = args.get("async").and_then(|v| v.as_bool()).unwrap_or(false);
        
        // Check if this is a batch invocation
        if args.get("agents").is_some() {
            if is_async {
                self.call_multiple_async(args, ctx).await
            } else {
                self.call_multiple(args, ctx).await
            }
        } else {
            if is_async {
                self.call_single_async(args, ctx).await
            } else {
                self.call_single(args, ctx).await
            }
        }
    }
}

impl InvokeAgentTool {
    /// Invoke a single agent (existing logic, now with global concurrency control)
    async fn call_single(
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

        // Acquire global semaphore permit to limit concurrency
        let _permit = INVOKE_AGENT_SEMAPHORE.acquire().await.map_err(|e| {
            ToolSourceError::Transport(format!("failed to acquire semaphore: {}", e))
        })?;

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

        let outcome = runner
            .stream_with_config(task, None, on_event)
            .await
            .map_err(|e| {
                ToolSourceError::Transport(format!("sub-agent '{}' failed: {}", agent_name, e))
            })?;

        let reply = match outcome {
            crate::runner_common::StreamRunOutcome::Finished(final_state) => final_state
                .last_assistant_reply()
                .unwrap_or_else(|| "(no reply from sub-agent)".to_string()),
            crate::runner_common::StreamRunOutcome::Cancelled => {
                "(sub-agent cancelled)".to_string()
            }
        };

        Ok(ToolCallContent { text: reply })
    }

    /// Invoke a single agent asynchronously (fire-and-forget)
    async fn call_single_async(
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

        // Validate agent exists before spawning
        let profile = resolve_profile(agent_name).map_err(|e| {
            ToolSourceError::InvalidInput(format!(
                "failed to resolve agent '{}': {}",
                agent_name, e
            ))
        })?;

        // Clone necessary data for background task
        let profile_clone = profile.clone();
        let base_config = self.base_config.clone();
        let max_depth = self.max_depth;
        let agent_name_str = agent_name.to_string();
        let task_str = task.to_string();
        let ctx_clone = ctx.cloned();
        let working_folder_clone = working_folder_override.clone();

        // Spawn background task
        tokio::spawn(async move {
            // Acquire global semaphore permit
            let _permit = match INVOKE_AGENT_SEMAPHORE.acquire().await {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(
                        agent = %agent_name_str,
                        error = %e,
                        "failed to acquire semaphore for async agent invocation"
                    );
                    return;
                }
            };

            // Build config and runner in background
            let mut sub_config = build_config_from_profile(
                &profile_clone,
                &base_config,
                working_folder_clone.as_deref(),
            );
            sub_config.thread_id = None;

            let runner = match build_react_runner(&sub_config, None, false, None).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(
                        agent = %agent_name_str,
                        error = %e,
                        "failed to build sub-agent for async invocation"
                    );
                    return;
                }
            };

            // Run agent in background
            let on_event = ctx_clone.and_then(|c| c.stream_writer.clone()).map(|writer| {
                let agent = agent_name_str.clone();
                move |event: crate::stream::StreamEvent<crate::state::ReActState>| {
                    let payload = serde_json::json!({
                        "sub_agent": agent,
                        "event": format!("{:?}", event),
                    });
                    writer.emit_custom(payload);
                }
            });

            match runner.stream_with_config(&task_str, None, on_event).await {
                Ok(outcome) => {
                    match outcome {
                        crate::runner_common::StreamRunOutcome::Finished(_) => {
                            tracing::info!(
                                agent = %agent_name_str,
                                "async agent invocation completed successfully"
                            );
                        }
                        crate::runner_common::StreamRunOutcome::Cancelled => {
                            tracing::warn!(
                                agent = %agent_name_str,
                                "async agent invocation was cancelled"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        agent = %agent_name_str,
                        error = %e,
                        "async agent invocation failed"
                    );
                }
            }
        });

        Ok(ToolCallContent {
            text: format!(
                "Agent '{}' started in background. Task: {}",
                agent_name, task
            ),
        })
    }

    /// Invoke multiple agents concurrently with global concurrency limit
    async fn call_multiple(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let agents = args.get("agents").and_then(|v| v.as_array()).ok_or_else(|| {
            ToolSourceError::InvalidInput("agents must be an array".into())
        })?;

        if agents.is_empty() {
            return Err(ToolSourceError::InvalidInput(
                "agents array cannot be empty".into(),
            ));
        }

        let fail_fast = args.get("fail_fast").and_then(|v| v.as_bool()).unwrap_or(false);

        // Validate all agent specs before spawning tasks
        for (idx, agent_spec) in agents.iter().enumerate() {
            if agent_spec.get("agent").and_then(|v| v.as_str()).is_none() {
                return Err(ToolSourceError::InvalidInput(format!(
                    "agent spec at index {} missing required field: agent",
                    idx
                )));
            }
            if agent_spec.get("task").and_then(|v| v.as_str()).is_none() {
                return Err(ToolSourceError::InvalidInput(format!(
                    "agent spec at index {} missing required field: task",
                    idx
                )));
            }
        }

        // Spawn concurrent tasks for each agent
        let mut handles = vec![];
        for agent_spec in agents {
            let args = agent_spec.clone();
            let ctx = ctx.cloned();
            let base_config = self.base_config.clone();
            let max_depth = self.max_depth;

            let handle = tokio::spawn(async move {
                // Acquire global semaphore permit
                let _permit = INVOKE_AGENT_SEMAPHORE.acquire().await.map_err(|e| {
                    ToolSourceError::Transport(format!("failed to acquire semaphore: {}", e))
                })?;

                // Invoke single agent
                invoke_single_agent(&base_config, args, ctx.as_ref(), max_depth).await
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete
        let results = futures::future::join_all(handles).await;

        // Aggregate results
        let mut successful = vec![];
        let mut failed = vec![];

        for (idx, result) in results.into_iter().enumerate() {
            match result {
                Ok(Ok(content)) => successful.push((idx, content.text)),
                Ok(Err(e)) => {
                    if fail_fast {
                        return Err(ToolSourceError::Transport(format!(
                            "agent {} failed (fail-fast mode): {}",
                            idx, e
                        )));
                    }
                    failed.push((idx, e.to_string()));
                }
                Err(e) => {
                    if fail_fast {
                        return Err(ToolSourceError::Transport(format!(
                            "agent {} panicked (fail-fast mode): {}",
                            idx, e
                        )));
                    }
                    failed.push((idx, format!("panic: {}", e)));
                }
            }
        }

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

        Ok(ToolCallContent { text: output })
    }

    /// Invoke multiple agents asynchronously (fire-and-forget)
    async fn call_multiple_async(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let agents = args.get("agents").and_then(|v| v.as_array()).ok_or_else(|| {
            ToolSourceError::InvalidInput("agents must be an array".into())
        })?;

        if agents.is_empty() {
            return Err(ToolSourceError::InvalidInput(
                "agents array cannot be empty".into(),
            ));
        }

        // Validate all agent specs before spawning tasks
        let mut agent_names = vec![];
        for (idx, agent_spec) in agents.iter().enumerate() {
            if agent_spec.get("agent").and_then(|v| v.as_str()).is_none() {
                return Err(ToolSourceError::InvalidInput(format!(
                    "agent spec at index {} missing required field: agent",
                    idx
                )));
            }
            if agent_spec.get("task").and_then(|v| v.as_str()).is_none() {
                return Err(ToolSourceError::InvalidInput(format!(
                    "agent spec at index {} missing required field: task",
                    idx
                )));
            }
            if let Some(name) = agent_spec.get("agent").and_then(|v| v.as_str()) {
                // Validate agent exists
                resolve_profile(name).map_err(|e| {
                    ToolSourceError::InvalidInput(format!(
                        "failed to resolve agent '{}' at index {}: {}",
                        name, idx, e
                    ))
                })?;
                agent_names.push(name.to_string());
            }
        }

        // Spawn all agents in background
        let base_config = self.base_config.clone();
        let max_depth = self.max_depth;
        let ctx_clone = ctx.cloned();

        for agent_spec in agents {
            let args = agent_spec.clone();
            let ctx = ctx_clone.clone();
            let base_config = base_config.clone();
            let max_depth = max_depth;

            tokio::spawn(async move {
                // Acquire global semaphore permit
                let _permit = match INVOKE_AGENT_SEMAPHORE.acquire().await {
                    Ok(p) => p,
                    Err(e) => {
                        if let Some(agent_name) = args.get("agent").and_then(|v| v.as_str()) {
                            tracing::error!(
                                agent = %agent_name,
                                error = %e,
                                "failed to acquire semaphore for async agent invocation"
                            );
                        }
                        return;
                    }
                };

                // Invoke single agent in background
                if let Err(e) = invoke_single_agent(&base_config, args, ctx.as_ref(), max_depth).await {
                    if let Some(agent_name) = e.to_string().split("'").nth(1) {
                        tracing::error!(
                            agent = %agent_name,
                            error = %e,
                            "async agent invocation failed"
                        );
                    }
                }
            });
        }

        Ok(ToolCallContent {
            text: format!(
                "Started {} agent(s) in background: {}",
                agent_names.len(),
                agent_names.join(", ")
            ),
        })
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
    if current_depth >= max_depth {
        return Err(ToolSourceError::InvalidInput(format!(
            "max sub-agent depth ({}) reached; cannot invoke further agents",
            max_depth,
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
        base_config,
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

    let outcome = runner
        .stream_with_config::<fn(_)>(task, None, None)
        .await
        .map_err(|e| {
            ToolSourceError::Transport(format!("sub-agent '{}' failed: {}", agent_name, e))
        })?;

    let reply = match outcome {
        crate::runner_common::StreamRunOutcome::Finished(final_state) => final_state
            .last_assistant_reply()
            .unwrap_or_else(|| "(no reply from sub-agent)".to_string()),
        crate::runner_common::StreamRunOutcome::Cancelled => {
            "(sub-agent cancelled)".to_string()
        }
    };

    Ok(ToolCallContent { text: reply })
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
        assert!(result.unwrap_err().to_string().contains("must be an array"));
    }
}
