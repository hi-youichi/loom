//! Batch invocation helper: worktree isolation lifecycle management.

use std::sync::Arc;

use serde_json::Value;

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::profile::resolve_profile;
use crate::agent::ReactBuildConfig;

use super::runner::build_and_run_sub_agent;

/// Invoke a single agent in batch context.
///
/// Handles depth checking, worktree isolation, then delegates to
/// [`build_and_run_sub_agent`] for the shared execution path.
pub(super) async fn invoke_single_agent(
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

    // Parse isolation and estimated_paths from tool call arguments
    let isolation_arg = args
        .get("isolation")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let estimated_paths: Vec<String> = args
        .get("estimated_paths")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    tracing::info!(
        agent = %agent_name,
        task_length = task.len(),
        depth = current_depth,
        isolation = ?isolation_arg,
        estimated_paths_count = estimated_paths.len(),
        "Starting single agent invocation"
    );

    let working_folder_override = args
        .get("working_folder")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);

    // --- resolve profile (needed for worktree config) ---
    tracing::debug!(agent = %agent_name, "Resolving agent profile");
    let profile = resolve_profile(agent_name).map_err(|e| {
        tracing::error!(agent = %agent_name, error = %e, "Failed to resolve agent profile");
        ToolSourceError::InvalidInput(format!("failed to resolve agent '{}': {}", agent_name, e))
    })?;

    // --- worktree isolation ---
    let use_worktree = isolation_arg.as_deref() == Some("worktree")
        || (isolation_arg.is_none() && profile.isolation.as_deref() == Some("worktree"));

    let worktree_handle = if use_worktree && working_folder_override.is_none() {
        let worktree_config = profile
            .worktree
            .as_ref()
            .map(|wc| wc.to_worktree_config())
            .unwrap_or_default();
        let current_dir = base_config
            .working_folder
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new("."));

        match worktree::WorktreeManager::from_working_dir(current_dir, worktree_config) {
            Ok(manager) => {
                match manager
                    .create_for_agent(
                        agent_name,
                        None,
                        if estimated_paths.is_empty() {
                            None
                        } else {
                            Some(&estimated_paths)
                        },
                    )
                    .await
                {
                    Ok(handle) => {
                        tracing::info!(
                            agent = %agent_name,
                            worktree_path = %handle.path.display(),
                            branch = ?handle.branch,
                            "Created worktree for isolated agent execution"
                        );
                        Some(handle)
                    }
                    Err(e) => {
                        tracing::warn!(
                            agent = %agent_name,
                            error = %e,
                            "Failed to create worktree, falling back to shared directory"
                        );
                        None
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    agent = %agent_name,
                    error = %e,
                    "Cannot init worktree manager, falling back to shared directory"
                );
                None
            }
        }
    } else {
        None
    };

    let effective_working_folder = worktree_handle
        .as_ref()
        .map(|h| h.path.clone())
        .or(working_folder_override);

    // --- delegate to shared execution ---
    let result = build_and_run_sub_agent(
        base_config,
        agent_name,
        task,
        &args,
        effective_working_folder.as_deref(),
        ctx,
    )
    .await;

    // --- worktree cleanup ---
    if let Some(handle) = worktree_handle {
        let wt_config = profile
            .worktree
            .as_ref()
            .map(|wc| wc.to_worktree_config())
            .unwrap_or_default();
        let manager = worktree::WorktreeManager::new(handle.repo_root.clone(), wt_config.clone());
        let has_changes = manager.check_changes(&handle).await.unwrap_or(false);

        if !has_changes && wt_config.auto_cleanup {
            tracing::info!(
                agent = %agent_name,
                worktree_path = %handle.path.display(),
                "Auto-cleaning worktree (no changes)"
            );
            if let Err(e) = manager.cleanup(handle).await {
                tracing::warn!(agent = %agent_name, error = %e, "Worktree cleanup failed");
            }
        } else if has_changes {
            tracing::info!(
                agent = %agent_name,
                worktree_path = %handle.path.display(),
                branch = ?handle.branch,
                "Worktree has changes — preserving for review/merge"
            );
        }
    }

    result
}
