use serde_json::Value;

use super::*;
use super::{ExtensionContext, ExtensionError};

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

    let mut args = vec!["merge"];
    if strategy == "squash" {
        args.push("--squash");
    }
    if no_fast_forward || strategy == "squash" {
        args.push("--no-ff");
    }
    args.push(&branch);

    let output = run_git(ctx, &args).await;
    match output {
        Ok(out) => {
            if strategy == "squash" {
                run_git(ctx, &["commit", "--no-edit"]).await?;
            }
            let fast_forward = out.contains("Fast-forward") || out.contains("fast-forward");
            let merge_commit = if fast_forward {
                None
            } else {
                let sha = run_git(ctx, &["rev-parse", "HEAD"])
                    .await
                    .unwrap_or_default();
                Some(sha.trim().to_string())
            };
            Ok(serde_json::json!({
                "branch": branch,
                "merged": true,
                "fastForward": fast_forward,
                "mergeCommit": merge_commit,
            }))
        }
        Err(e) => {
            if matches!(e.code, -32603) {
                let conflicts = get_conflict_files(ctx).await;
                Err(ExtensionError {
                    code: -32602,
                    message: "invalid_params".into(),
                    data: Some(serde_json::json!({"conflicts": conflicts})),
                })
            } else {
                Err(e)
            }
        }
    }
}

pub async fn handle_merge_abort(
    _params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    if !check_in_progress(ctx, &["MERGE_HEAD"]).await {
        return Err(ExtensionError::invalid_params("no merge in progress"));
    }
    run_git(ctx, &["merge", "--abort"]).await?;
    Ok(serde_json::json!({"aborted": true}))
}

pub async fn handle_merge_continue(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let message: Option<String> = optional_param_str(&params, "message");

    if !check_in_progress(ctx, &["MERGE_HEAD"]).await {
        return Err(ExtensionError::invalid_params("no merge in progress"));
    }

    let msg;
    let args: Vec<&str> = if let Some(m) = message {
        msg = m;
        vec!["merge", "--continue", "-m", &msg]
    } else {
        vec!["merge", "--continue"]
    };

    match run_git(ctx, &args).await {
        Ok(_) => {
            let sha = run_git(ctx, &["rev-parse", "HEAD"])
                .await
                .unwrap_or_default();
            Ok(serde_json::json!({
                "continued": true,
                "mergeCommit": sha.trim(),
            }))
        }
        Err(e) => Err(e),
    }
}

pub async fn handle_rebase(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let branch: String = require_param(&params, "branch")?;
    let interactive: bool = params
        .get("interactive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut args = vec!["rebase"];
    if interactive {
        args.push("-i");
    }
    args.push(&branch);

    let output = run_git(ctx, &args).await;
    match output {
        Ok(_) => Ok(serde_json::json!({
            "branch": branch,
            "rebased": true,
            "conflicts": serde_json::json!([]),
        })),
        Err(e) => {
            if matches!(e.code, -32603) {
                let conflicts = get_conflict_files(ctx).await;
                Err(ExtensionError {
                    code: -32602,
                    message: "invalid_params".into(),
                    data: Some(serde_json::json!({"conflicts": conflicts})),
                })
            } else {
                Err(e)
            }
        }
    }
}

pub async fn handle_rebase_abort(
    _params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    if !check_in_progress(ctx, &["rebase-merge", "rebase-apply"]).await {
        return Err(ExtensionError::invalid_params("no rebase in progress"));
    }
    run_git(ctx, &["rebase", "--abort"]).await?;
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

    if !check_in_progress(ctx, &["rebase-merge", "rebase-apply"]).await {
        return Err(ExtensionError::invalid_params("no rebase in progress"));
    }

    let args: Vec<&str> = if skip {
        vec!["rebase", "--skip"]
    } else {
        vec!["rebase", "--continue"]
    };

    match run_git(ctx, &args).await {
        Ok(_) => {
            let remaining = if check_in_progress(ctx, &["rebase-merge", "rebase-apply"]).await {
                get_conflict_files(ctx).await.len()
            } else {
                0
            };
            Ok(serde_json::json!({
                "continued": true,
                "remainingConflicts": remaining,
            }))
        }
        Err(e) => Err(e),
    }
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

    match run_git(ctx, &["cherry-pick", &commit_sha]).await {
        Ok(_) => {
            let new_sha = run_git(ctx, &["rev-parse", "HEAD"])
                .await
                .unwrap_or_default();
            Ok(serde_json::json!({
                "commitSha": commit_sha,
                "cherryPicked": true,
                "newCommitSha": new_sha.trim(),
            }))
        }
        Err(e) => {
            if matches!(e.code, -32603) {
                let conflicts = get_conflict_files(ctx).await;
                Err(ExtensionError {
                    code: -32602,
                    message: "invalid_params".into(),
                    data: Some(serde_json::json!({"conflicts": conflicts})),
                })
            } else {
                Err(e)
            }
        }
    }
}

pub async fn handle_revert_commit(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let commit_sha: String = require_param(&params, "commitSha")?;

    match run_git(ctx, &["revert", "--no-edit", &commit_sha]).await {
        Ok(_) => {
            let revert_sha = run_git(ctx, &["rev-parse", "HEAD"])
                .await
                .unwrap_or_default();
            Ok(serde_json::json!({
                "commitSha": commit_sha,
                "reverted": true,
                "revertCommitSha": revert_sha.trim(),
            }))
        }
        Err(e) => {
            if matches!(e.code, -32603) {
                let conflicts = get_conflict_files(ctx).await;
                Err(ExtensionError {
                    code: -32602,
                    message: "invalid_params".into(),
                    data: Some(serde_json::json!({"conflicts": conflicts})),
                })
            } else {
                Err(e)
            }
        }
    }
}

pub async fn handle_reset_to_commit(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let commit_sha: String = require_param(&params, "commitSha")?;
    let mode: String = require_param(&params, "mode")?;

    let mode_arg = match mode.as_str() {
        "soft" => "--soft",
        "mixed" => "--mixed",
        "hard" => {
            require_git_scope(ctx, "git:destructive")?;
            "--hard"
        }
        _ => {
            return Err(ExtensionError::invalid_params(
                "mode must be 'soft', 'mixed', or 'hard'",
            ))
        }
    };

    require_git_scope(ctx, "git:history")?;
    run_git(ctx, &["reset", mode_arg, &commit_sha]).await?;
    Ok(serde_json::json!({
        "commitSha": commit_sha,
        "mode": mode,
        "reset": true,
    }))
}

async fn get_conflict_files(ctx: &ExtensionContext) -> Vec<String> {
    let output = run_git(ctx, &["diff", "--name-only", "--diff-filter=U"])
        .await
        .unwrap_or_default();
    output
        .lines()
        .filter(|l| !l.is_empty())
        .map(|s| s.to_string())
        .collect()
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
