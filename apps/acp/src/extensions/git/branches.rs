use serde_json::Value;

use super::*;
use super::{ExtensionContext, ExtensionError};

pub async fn handle_branches(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let remote: bool = params
        .get("remote")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let cursor: Option<String> = params
        .get("cursor")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let limit: usize = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(50);

    let skip = decode_cursor_offset(&cursor);

    let repo_dir = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let branches = anureo_git::facade::branches(&repo_dir, remote)
        .await
        .map_err(ext_err_from_git)?;

    let total = branches.len();
    let end = (skip + limit).min(total);
    let items = branches[skip..end].to_vec();
    let has_more = end < total;
    let next_cursor = if has_more {
        encode_cursor_offset(end)
    } else {
        None
    };

    Ok(serde_json::json!({
        "items": items,
        "nextCursor": next_cursor,
        "hasMore": has_more,
    }))
}

pub async fn handle_checkout_branch(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let branch: String = require_param(&params, "branch")?;
    let prev = run_git(ctx, &["rev-parse", "--abbrev-ref", "HEAD"])
        .await
        .unwrap_or_default();
    match run_git(ctx, &["checkout", &branch]).await {
        Ok(_) => Ok(serde_json::json!({
            "branch": branch,
            "previousBranch": prev.trim(),
            "checkedOut": true,
        })),
        Err(e) => {
            if matches!(e.code, -32603) {
                Err(ExtensionError::invalid_params(
                    "cannot checkout: dirty worktree or branch issue".to_string(),
                ))
            } else {
                Err(e)
            }
        }
    }
}

pub async fn handle_create_branch(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let branch: String = require_param(&params, "branch")?;
    let base_ref: String = require_param(&params, "baseRef")?;
    let checkout: bool = params
        .get("checkout")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let args: Vec<&str> = if checkout {
        vec!["checkout", "-b", &branch, &base_ref]
    } else {
        vec!["branch", &branch, &base_ref]
    };

    match run_git(ctx, &args).await {
        Ok(_) => {
            let base_commit = run_git(ctx, &["rev-parse", "--short", &base_ref])
                .await
                .unwrap_or_default();
            Ok(serde_json::json!({
                "branch": branch,
                "baseCommit": base_commit.trim(),
                "created": true,
            }))
        }
        Err(e) => {
            if matches!(e.code, -32603) {
                Err(ExtensionError {
                    code: -32005,
                    message: "conflict".into(),
                    data: Some(Value::String(format!("branch '{branch}' already exists"))),
                })
            } else {
                Err(e)
            }
        }
    }
}

pub async fn handle_rename_branch(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let old_name: String = require_param(&params, "oldName")?;
    let new_name: String = require_param(&params, "newName")?;
    run_git(ctx, &["branch", "-m", &old_name, &new_name]).await?;
    Ok(serde_json::json!({"oldName": old_name, "newName": new_name, "renamed": true}))
}

pub async fn handle_delete_branch(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let branch: String = require_param(&params, "branch")?;
    let force: bool = params
        .get("force")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let current = run_git(ctx, &["rev-parse", "--abbrev-ref", "HEAD"])
        .await
        .unwrap_or_default();
    if current.trim() == branch {
        return Err(ExtensionError::forbidden(
            "cannot delete the currently checked out branch",
        ));
    }

    let flag = if force { "-D" } else { "-d" };
    match run_git(ctx, &["branch", flag, &branch]).await {
        Ok(_) => Ok(serde_json::json!({"branch": branch, "deleted": true})),
        Err(e) => {
            if matches!(e.code, -32603) {
                if !force {
                    Err(ExtensionError::invalid_params(format!(
                        "branch '{branch}' is not fully merged; use force=true to delete"
                    )))
                } else {
                    Err(e)
                }
            } else {
                Err(e)
            }
        }
    }
}

pub async fn handle_checkout_commit(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:history")?;
    let commit_sha: String = require_param(&params, "commitSha")?;
    match run_git(ctx, &["checkout", &commit_sha]).await {
        Ok(_) => Ok(serde_json::json!({"commitSha": commit_sha, "detachedHead": true})),
        Err(e) => {
            if matches!(e.code, -32603) {
                Err(ExtensionError::invalid_params(
                    "cannot checkout: dirty worktree",
                ))
            } else {
                Err(e)
            }
        }
    }
}
