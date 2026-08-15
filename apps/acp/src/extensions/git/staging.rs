use serde_json::Value;

use super::*;
use super::{ExtensionContext, ExtensionError};

pub async fn handle_stage_file(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let file_path: String = require_param(&params, "filePath")?;
    validate_git_path(&file_path, ctx)?;
    run_git(ctx, &["add", &file_path]).await?;
    Ok(serde_json::json!({"filePath": file_path, "staged": true}))
}

pub async fn handle_stage_files(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let file_paths: Vec<String> = require_param(&params, "filePaths")?;
    let mut staged = Vec::new();
    let mut failed = Vec::new();
    for fp in &file_paths {
        if validate_git_path(fp, ctx).is_err() {
            failed.push(fp.clone());
            continue;
        }
        match run_git(ctx, &["add", fp]).await {
            Ok(_) => staged.push(fp.clone()),
            Err(_) => failed.push(fp.clone()),
        }
    }
    if staged.is_empty() {
        return Err(ExtensionError::invalid_params("all paths are invalid"));
    }
    Ok(serde_json::json!({"staged": staged, "failed": failed}))
}

pub async fn handle_unstage_file(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let file_path: String = require_param(&params, "filePath")?;
    validate_git_path(&file_path, ctx)?;
    run_git(ctx, &["restore", "--staged", &file_path]).await?;
    Ok(serde_json::json!({"filePath": file_path, "unstaged": true}))
}

pub async fn handle_unstage_files(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let file_paths: Vec<String> = require_param(&params, "filePaths")?;
    let mut unstaged = Vec::new();
    let mut failed = Vec::new();
    for fp in &file_paths {
        if validate_git_path(fp, ctx).is_err() {
            failed.push(fp.clone());
            continue;
        }
        match run_git(ctx, &["restore", "--staged", fp]).await {
            Ok(_) => unstaged.push(fp.clone()),
            Err(_) => failed.push(fp.clone()),
        }
    }
    Ok(serde_json::json!({"unstaged": unstaged, "failed": failed}))
}

pub async fn handle_stage_hunk(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let file_path: String = require_param(&params, "filePath")?;
    let patch: String = require_param(&params, "patch")?;
    validate_git_path(&file_path, ctx)?;
    run_git_apply(ctx, &["apply", "--cached", "--whitespace=nowarn"], &patch).await?;
    Ok(serde_json::json!({"filePath": file_path, "staged": true}))
}

pub async fn handle_unstage_hunk(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let file_path: String = require_param(&params, "filePath")?;
    let patch: String = require_param(&params, "patch")?;
    validate_git_path(&file_path, ctx)?;
    run_git_apply(
        ctx,
        &["apply", "--cached", "--reverse", "--whitespace=nowarn"],
        &patch,
    )
    .await?;
    Ok(serde_json::json!({"filePath": file_path, "unstaged": true}))
}

pub async fn handle_revert_file(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let file_path: String = require_param(&params, "filePath")?;
    let scope: String = params
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("working")
        .to_string();
    validate_git_path(&file_path, ctx)?;

    match scope.as_str() {
        "working" => {
            run_git(ctx, &["checkout", "--", &file_path]).await?;
        }
        "all" => {
            run_git(ctx, &["restore", "--staged", "--worktree", &file_path]).await?;
        }
        _ => {
            return Err(ExtensionError::invalid_params(
                "scope must be 'working' or 'all'",
            ))
        }
    }
    Ok(serde_json::json!({"filePath": file_path, "reverted": true}))
}

pub async fn handle_revert_hunk(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let file_path: String = require_param(&params, "filePath")?;
    let patch: String = require_param(&params, "patch")?;
    validate_git_path(&file_path, ctx)?;
    run_git_apply(ctx, &["apply", "--reverse", "--whitespace=nowarn"], &patch).await?;
    Ok(serde_json::json!({"filePath": file_path, "reverted": true}))
}

pub async fn handle_commit(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:commit")?;
    let message: String = require_param(&params, "message")?;
    let amend: bool = params
        .get("amend")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let signoff: bool = params
        .get("signoff")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let staged_check = run_git(ctx, &["diff", "--cached", "--name-only"]).await?;
    if staged_check.trim().is_empty() {
        return Err(ExtensionError::invalid_params("no staged changes"));
    }

    let mut args = vec!["commit", "-m", &message];
    if amend {
        args.push("--amend");
    }
    if signoff {
        args.push("--signoff");
    }
    let _ = run_git(ctx, &args).await?;

    let sha = run_git(ctx, &["rev-parse", "HEAD"]).await?;
    let branch = run_git(ctx, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    let stat = run_git(ctx, &["show", "--stat", "--format=", "HEAD"]).await?;

    let (insertions, deletions, files_changed) = parse_commit_stat(&stat);

    Ok(serde_json::json!({
        "sha": sha.trim(),
        "branch": branch.trim(),
        "message": message,
        "filesChanged": files_changed,
        "insertions": insertions,
        "deletions": deletions,
    }))
}

fn parse_commit_stat(stat: &str) -> (u32, u32, u32) {
    let mut files_changed = 0u32;
    let mut insertions = 0u32;
    let mut deletions = 0u32;

    for line in stat.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.contains("file") || trimmed.contains("insertion") || trimmed.contains("deletion")
        {
            let mut last_num = 0u32;
            for token in trimmed.split_whitespace() {
                if let Ok(n) = token.parse::<u32>() {
                    last_num = n;
                } else if token.contains("file") {
                    files_changed = last_num;
                } else if token.contains("insertion") {
                    insertions = last_num;
                } else if token.contains("deletion") {
                    deletions = last_num;
                }
            }
            break;
        }
        files_changed += 1;
    }
    (insertions, deletions, files_changed)
}
