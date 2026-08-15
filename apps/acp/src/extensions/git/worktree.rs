use serde_json::Value;
use std::path::Path;

use super::*;
use super::{ExtensionContext, ExtensionError};

pub async fn handle_validate_worktree_directory(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let path: String = require_param(&params, "path")?;

    if path.is_empty() {
        return Err(ExtensionError::invalid_params("path is empty"));
    }
    if path.contains("..") {
        return Err(ExtensionError::invalid_params(format!(
            "path contains '..': {path}"
        )));
    }

    let base = ctx
        .working_directory
        .as_deref()
        .unwrap_or_else(|| Path::new("."));
    let path_obj = std::path::Path::new(&path);
    let resolved = if path_obj.is_absolute() {
        path_obj.to_path_buf()
    } else {
        base.join(&path)
    };

    let base_canonical = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    let resolved_canonical = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());

    if !resolved_canonical.starts_with(&base_canonical) {
        return Err(ExtensionError::invalid_params(format!(
            "path outside worktree root: {path}"
        )));
    }

    let normalized = resolved_canonical.to_string_lossy().to_string();
    let worktree_root = base_canonical.to_string_lossy().to_string();

    Ok(serde_json::json!({
        "path": path,
        "valid": true,
        "worktreeRoot": worktree_root,
        "normalizedPath": normalized,
    }))
}

pub async fn handle_canonicalize_worktree_state(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let path: String = require_param(&params, "path")?;
    let _ = path;

    let output = run_git(ctx, &["status", "--porcelain=v2", "--branch"]).await?;
    let status = parse_porcelain_status_v2(&output);

    let is_detached = status.branch == "(detached)" || status.branch == "HEAD";
    let is_dirty = !status.files.is_empty();
    let is_merge = check_in_progress(ctx, &["MERGE_HEAD"]).await;
    let is_rebase = check_in_progress(ctx, &["rebase-merge", "rebase-apply"]).await;

    let (state, attention_reason) = if is_merge {
        (
            "merge_in_progress",
            Some("merge in progress with conflicts".to_string()),
        )
    } else if is_rebase {
        ("rebase_in_progress", Some("rebase in progress".to_string()))
    } else if is_detached {
        ("detached", Some("detached HEAD state".to_string()))
    } else if is_dirty {
        (
            "dirty",
            Some("uncommitted changes in working directory".to_string()),
        )
    } else {
        ("clean", None)
    };

    let branch = run_git(ctx, &["rev-parse", "--abbrev-ref", "HEAD"])
        .await
        .unwrap_or_default();
    let head = run_git(ctx, &["rev-parse", "HEAD"])
        .await
        .unwrap_or_default();

    Ok(serde_json::json!({
        "path": path,
        "branch": branch.trim(),
        "head": head.trim(),
        "attentionReason": attention_reason,
        "state": state,
    }))
}

pub async fn handle_is_linked_worktree(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let path: String = require_param(&params, "path")?;
    let _ = path;

    let base = ctx
        .working_directory
        .as_deref()
        .unwrap_or_else(|| Path::new("."));
    let git_dir = base.join(".git");

    let is_linked = if git_dir.is_file() {
        if let Ok(content) = std::fs::read_to_string(&git_dir) {
            content.starts_with("gitdir:")
        } else {
            false
        }
    } else {
        false
    };

    let main_worktree = if is_linked {
        run_git(ctx, &["rev-parse", "--git-common-dir"])
            .await
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        base.to_string_lossy().to_string()
    };

    Ok(serde_json::json!({
        "path": path,
        "isLinked": is_linked,
        "mainWorktree": main_worktree,
    }))
}
