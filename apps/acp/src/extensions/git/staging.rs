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
    let repo_dir = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    loom_git::facade::stage_file(&repo_dir, &file_path)
        .await
        .map_err(ext_err_from_git)?;
    Ok(serde_json::json!({"filePath": file_path, "staged": true}))
}

pub async fn handle_stage_files(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let file_paths: Vec<String> = require_param(&params, "filePaths")?;
    let repo_dir = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut staged = Vec::new();
    let mut failed = Vec::new();
    for fp in &file_paths {
        if validate_git_path(fp, ctx).is_err() {
            failed.push(fp.clone());
            continue;
        }
        match loom_git::facade::stage_file(&repo_dir, fp).await {
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
    let repo_dir = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    loom_git::facade::unstage_file(&repo_dir, &file_path)
        .await
        .map_err(ext_err_from_git)?;
    Ok(serde_json::json!({"filePath": file_path, "unstaged": true}))
}

pub async fn handle_unstage_files(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let file_paths: Vec<String> = require_param(&params, "filePaths")?;
    let repo_dir = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut unstaged = Vec::new();
    let mut failed = Vec::new();
    for fp in &file_paths {
        if validate_git_path(fp, ctx).is_err() {
            failed.push(fp.clone());
            continue;
        }
        match loom_git::facade::unstage_file(&repo_dir, fp).await {
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

    let repo_dir = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let req = loom_git::types::CommitRequest {
        message,
        amend,
        signoff,
    };
    let result = loom_git::facade::commit(&repo_dir, req)
        .await
        .map_err(ext_err_from_git)?;

    Ok(serde_json::json!({
        "sha": result.sha,
        "branch": result.branch,
        "message": result.message,
        "filesChanged": result.files_changed,
        "insertions": result.insertions,
        "deletions": result.deletions,
        "unsigned": result.unsigned,
    }))
}
