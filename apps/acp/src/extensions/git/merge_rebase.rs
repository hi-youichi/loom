use serde_json::Value;

use super::*;
use super::{ExtensionContext, ExtensionError};

fn repo_dir(ctx: &ExtensionContext) -> std::path::PathBuf {
    ctx.working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

fn conflict_payload(conflicts: Vec<String>) -> ExtensionError {
    ExtensionError {
        code: -32602,
        message: "invalid_params".into(),
        data: Some(serde_json::json!({"conflicts": conflicts})),
    }
}

pub async fn handle_merge(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let branch: String = require_param(&params, "branch")?;
    let strategy: String = params
        .get("strategy")
        .and_then(|v| v.as_str())
        .unwrap_or("merge")
        .to_string();
    let no_fast_forward: bool = params
        .get("noFastForward")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let squash = strategy == "squash";
    let opts = loom_git::types::MergeOptions {
        squash,
        no_ff: no_fast_forward || squash,
        message: None,
    };
    let result = loom_git::facade::merge(&repo_dir(ctx), &branch, opts)
        .await
        .map_err(ext_err_from_git)?;

    if result.conflicted {
        return Err(conflict_payload(result.conflicts));
    }
    Ok(serde_json::json!({
        "branch": branch,
        "merged": true,
        "fastForward": result.fast_forward,
        "mergeCommit": result.merge_commit,
    }))
}

pub async fn handle_merge_abort(
    _params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let in_progress = loom_git::facade::in_progress(&repo_dir(ctx))
        .await
        .unwrap_or(None);
    let merging = in_progress
        .as_ref()
        .map(|ip| matches!(ip.operation, loom_git::types::GitOperation::Merge))
        .unwrap_or(false);
    if !merging {
        return Err(ExtensionError::invalid_params("no merge in progress"));
    }
    loom_git::facade::merge_abort(&repo_dir(ctx))
        .await
        .map_err(ext_err_from_git)?;
    Ok(serde_json::json!({"aborted": true}))
}

pub async fn handle_merge_continue(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let message: Option<String> = optional_param_str(&params, "message");

    let in_progress = loom_git::facade::in_progress(&repo_dir(ctx))
        .await
        .unwrap_or(None);
    let merging = in_progress
        .as_ref()
        .map(|ip| matches!(ip.operation, loom_git::types::GitOperation::Merge))
        .unwrap_or(false);
    if !merging {
        return Err(ExtensionError::invalid_params("no merge in progress"));
    }

    // bug#2 fix: the explicit message is passed through to the commit that
    // concludes the merge (the old `merge --continue -m` dropped it).
    let result = loom_git::facade::merge_continue(&repo_dir(ctx), message.as_deref())
        .await
        .map_err(ext_err_from_git)?;
    if result.conflicted {
        return Err(conflict_payload(result.conflicts));
    }
    Ok(serde_json::json!({
        "continued": true,
        "mergeCommit": result.merge_commit.unwrap_or_default(),
    }))
}

pub async fn handle_rebase(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let branch: String = require_param(&params, "branch")?;
    // bug#1 fix: the `interactive` flag previously caused `rebase -i` to hang
    // waiting on an editor. The facade drives rebases non-interactively via
    // parameter lists / the Rebase API, so the flag is now a no-op hint.
    let _interactive: bool = params
        .get("interactive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let result = loom_git::facade::rebase(&repo_dir(ctx), &branch)
        .await
        .map_err(ext_err_from_git)?;
    if result.conflicted {
        return Err(conflict_payload(result.conflicts));
    }
    Ok(serde_json::json!({
        "branch": branch,
        "rebased": true,
        "conflicts": serde_json::json!([]),
    }))
}

pub async fn handle_rebase_abort(
    _params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let in_progress = loom_git::facade::in_progress(&repo_dir(ctx))
        .await
        .unwrap_or(None);
    let rebasing = in_progress
        .as_ref()
        .map(|ip| matches!(ip.operation, loom_git::types::GitOperation::Rebase))
        .unwrap_or(false);
    if !rebasing {
        return Err(ExtensionError::invalid_params("no rebase in progress"));
    }
    loom_git::facade::rebase_abort(&repo_dir(ctx))
        .await
        .map_err(ext_err_from_git)?;
    Ok(serde_json::json!({"aborted": true}))
}

pub async fn handle_rebase_continue(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let skip: bool = params
        .get("skip")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let in_progress = loom_git::facade::in_progress(&repo_dir(ctx))
        .await
        .unwrap_or(None);
    let rebasing = in_progress
        .as_ref()
        .map(|ip| matches!(ip.operation, loom_git::types::GitOperation::Rebase))
        .unwrap_or(false);
    if !rebasing {
        return Err(ExtensionError::invalid_params("no rebase in progress"));
    }

    let result = if skip {
        loom_git::facade::rebase_skip(&repo_dir(ctx))
            .await
            .map_err(ext_err_from_git)?
    } else {
        loom_git::facade::rebase_continue(&repo_dir(ctx), None)
            .await
            .map_err(ext_err_from_git)?
    };
    if result.conflicted {
        return Err(conflict_payload(result.conflicts));
    }
    let remaining = loom_git::facade::in_progress(&repo_dir(ctx))
        .await
        .ok()
        .flatten()
        .map(|ip| ip.conflict_files.len())
        .unwrap_or(0);
    Ok(serde_json::json!({
        "continued": true,
        "remainingConflicts": remaining,
    }))
}

pub async fn handle_conflict_details(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let _ = params;
    let operation = detect_operation(ctx).await;
    let conflict_files = get_conflict_files_detailed(ctx).await;

    let op_str = match operation {
        Some(GitOperation::Merge) => "merge",
        Some(GitOperation::Rebase) => "rebase",
        Some(GitOperation::CherryPick) => "cherry_pick",
        Some(GitOperation::Revert) => "revert",
        _ => "none",
    };

    Ok(serde_json::json!({
        "operation": op_str,
        "conflictFiles": conflict_files,
    }))
}

pub async fn handle_cherry_pick(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let commit_sha: String = require_param(&params, "commitSha")?;

    let result = loom_git::facade::cherry_pick(&repo_dir(ctx), &commit_sha, false)
        .await
        .map_err(ext_err_from_git)?;
    if result.conflicted {
        return Err(conflict_payload(result.conflicts));
    }
    Ok(serde_json::json!({
        "commitSha": commit_sha,
        "cherryPicked": true,
        "newCommitSha": result.merge_commit.unwrap_or_default(),
    }))
}

pub async fn handle_revert_commit(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let commit_sha: String = require_param(&params, "commitSha")?;

    let result = loom_git::facade::revert_commit(&repo_dir(ctx), &commit_sha)
        .await
        .map_err(ext_err_from_git)?;
    if result.conflicted {
        return Err(conflict_payload(result.conflicts));
    }
    Ok(serde_json::json!({
        "commitSha": commit_sha,
        "reverted": true,
        "revertCommitSha": result.merge_commit.unwrap_or_default(),
    }))
}

pub async fn handle_reset_to_commit(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let commit_sha: String = require_param(&params, "commitSha")?;
    let mode: String = require_param(&params, "mode")?;

    match mode.as_str() {
        "soft" | "mixed" | "hard" => {}
        _ => {
            return Err(ExtensionError::invalid_params(
                "mode must be 'soft', 'mixed', or 'hard'",
            ))
        }
    }
    if mode == "hard" {
        require_git_scope(ctx, "git:destructive")?;
    }
    require_git_scope(ctx, "git:history")?;
    loom_git::facade::reset_to_commit(&repo_dir(ctx), &commit_sha, &mode)
        .await
        .map_err(ext_err_from_git)?;
    Ok(serde_json::json!({
        "commitSha": commit_sha,
        "mode": mode,
        "reset": true,
    }))
}

async fn get_conflict_files(ctx: &ExtensionContext) -> Vec<String> {
    loom_git::facade::in_progress(&repo_dir(ctx))
        .await
        .ok()
        .flatten()
        .map(|ip| ip.conflict_files)
        .unwrap_or_default()
}

async fn get_conflict_files_detailed(ctx: &ExtensionContext) -> Vec<ConflictFile> {
    let files = get_conflict_files(ctx).await;
    let mut result = Vec::new();
    for path in &files {
        let content = ctx
            .working_directory
            .as_ref()
            .and_then(|d| std::fs::read_to_string(d.join(path)).ok())
            .unwrap_or_default();

        let mut lines = Vec::new();
        let mut current_kind = ConflictLineKind::Context;
        for line in content.lines() {
            if line.starts_with("<<<<<<< ") {
                current_kind = ConflictLineKind::Ours;
                lines.push(ConflictLine {
                    kind: ConflictLineKind::ConflictMarker,
                    content: line.to_string(),
                });
            } else if line.starts_with("=======") {
                current_kind = ConflictLineKind::Theirs;
                lines.push(ConflictLine {
                    kind: ConflictLineKind::ConflictMarker,
                    content: line.to_string(),
                });
            } else if line.starts_with(">>>>>>> ") {
                current_kind = ConflictLineKind::Context;
                lines.push(ConflictLine {
                    kind: ConflictLineKind::ConflictMarker,
                    content: line.to_string(),
                });
            } else {
                lines.push(ConflictLine {
                    kind: current_kind.clone(),
                    content: line.to_string(),
                });
            }
        }
        result.push(ConflictFile {
            path: path.clone(),
            hunks: vec![ConflictHunk {
                ours_start: 0,
                theirs_start: 0,
                lines,
            }],
        });
    }
    result
}
