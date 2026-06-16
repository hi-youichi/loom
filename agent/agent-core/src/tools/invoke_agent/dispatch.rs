//! Dispatch implementations: single / concurrent / async invocation modes.

use std::sync::Arc;

use serde_json::Value;

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};
use loom_react_config::profile::resolve_profile;

use super::InvokeAgentTool;
use super::runner::build_and_run_sub_agent;
use super::worktree::invoke_single_agent;

impl InvokeAgentTool {
    pub(super) async fn call_single(
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
        let working_folder_override = args
            .get("working_folder")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from);

        build_and_run_sub_agent(
            &self.base_config,
            agent_name,
            task,
            &args,
            working_folder_override.as_deref(),
            ctx,
        )
        .await
    }

    pub(super) async fn call_multiple(
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

    pub(super) async fn call_multiple_async(
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

        let base_config: Arc<_> = self.base_config.clone();
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
