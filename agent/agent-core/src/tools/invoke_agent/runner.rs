//! Core sub-agent execution: profile → config → tier resolution → runner.

use std::sync::Arc;

use serde_json::Value;

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};
use loom_react_config::build_config_from_profile;
use loom_react_config::profile::resolve_profile;
use loom_react_config::ReactBuildConfig;
use loom_react_config::resolve_tier_and_build_config;
use crate::agent::react::build::build_react_runner;

/// Build sub-agent config, resolve model tier, construct ReactRunner, and execute.
///
/// This is the single shared path for all agent invocations (single, concurrent,
/// async). Callers handle their own pre-steps (depth check, worktree setup) and
/// pass the resolved `working_folder` here.
pub(super) async fn build_and_run_sub_agent(
    base_config: &Arc<ReactBuildConfig>,
    agent_name: &str,
    task: &str,
    args: &Value,
    working_folder_override: Option<&std::path::Path>,
    ctx: Option<&ToolCallContext>,
) -> Result<ToolCallContent, ToolSourceError> {
    tracing::info!(
        agent = %agent_name,
        task_length = task.len(),
        depth = ctx.map(|c| c.depth).unwrap_or(0),
        "Starting execution of agent task"
    );

    if let Some(folder) = working_folder_override {
        tracing::debug!(
            agent = %agent_name,
            working_folder = %folder.display(),
            "Using custom working folder"
        );
    }

    // --- resolve profile ---
    tracing::debug!(agent = %agent_name, "Resolving agent profile");
    let profile = resolve_profile(agent_name).map_err(|e| {
        tracing::error!(agent = %agent_name, error = %e, "Failed to resolve agent profile");
        ToolSourceError::InvalidInput(format!("failed to resolve agent '{}': {}", agent_name, e))
    })?;

    // --- build sub config ---
    tracing::debug!(
        agent = %agent_name,
        profile_name = %profile.name,
        "Building sub-agent configuration"
    );
    let mut sub_config =
        build_config_from_profile(&profile, base_config, working_folder_override);

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

    // --- model_tier override ---
    if let Some(tier_str) = args.get("model_tier").and_then(|v| v.as_str()) {
        tracing::info!(
            agent = %agent_name,
            tier_override = %tier_str,
            current_profile_tier = ?sub_config.model_tier,
            current_profile_model = ?sub_config.model,
            "Processing model_tier override request"
        );
        match serde_json::from_str::<model_spec_core::ModelTier>(tier_str) {
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

    // --- thread isolation ---
    tracing::debug!(
        agent = %agent_name,
        final_tier_before_resolution = ?sub_config.model_tier,
        final_model_before_resolution = ?sub_config.model,
        final_provider_before_resolution = ?sub_config.llm_provider,
        "Final model configuration before tier resolution"
    );

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

    // --- tier resolution ---
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

    // --- build runner + run ---
    tracing::debug!(agent = %agent_name, "Building React runner");
    let child_depth = ctx.map_or(1u32, |c| c.depth + 1);
    let loom_sender = ctx.and_then(|c| c.any_stream_event_sender.clone());
    let cli_sender: Option<Arc<dyn Fn(loom_cli_types::AnyStreamEvent) + Send + Sync>> =
        loom_sender.as_ref().map(|s| {
            let s = s.clone();
            Arc::new(move |ev: loom_cli_types::AnyStreamEvent| {
                let val = serde_json::to_value(&ev).unwrap_or_default();
                s(loom_stream::AnyStreamEvent::React(val));
            }) as Arc<dyn Fn(loom_cli_types::AnyStreamEvent) + Send + Sync>
        });
    let runner = build_react_runner(&sub_config, None, false, None, cli_sender)
        .await
        .map_err(|e| {
            tracing::error!(agent = %agent_name, error = %e, "Failed to build sub-agent runner");
            ToolSourceError::Transport(format!(
                "failed to build sub-agent '{}': {}",
                agent_name, e
            ))
        })?;

    tracing::debug!(agent = %agent_name, "Starting sub-agent execution");
    let agent_name_for_event = agent_name.to_string();
    let start = std::time::Instant::now();

    let on_event = Some(move |event: loom_stream::StreamEvent<loom_cli_types::ReActState>| {
        match &event {
            loom_stream::StreamEvent::TaskStart { .. }
            | loom_stream::StreamEvent::TaskEnd { .. }
            | loom_stream::StreamEvent::ToolStart { .. }
            | loom_stream::StreamEvent::ToolEnd { .. } => {
                if let Some(formatted) = loom_stream_display::format_subagent_event(
                    &event,
                    &agent_name_for_event,
                    child_depth,
                    start,
                ) {
                    eprintln!("{}", formatted);
                }
            }
            _ => {}
        }
        if let Some(ref sender) = loom_sender {
            let val = serde_json::to_value(&event).unwrap_or_default();
            sender(loom_stream::AnyStreamEvent::React(val));
        }
    });

    let outcome = runner
        .stream_with_config(task, None, on_event)
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
