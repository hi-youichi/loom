use serde_json::Value;

use super::*;
use super::{ExtensionContext, ExtensionError};

pub async fn handle(
    params: Value,
    _method: &str,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    match _method {
        "stash_list" => handle_stash_list(params, ctx).await,
        "stash_create" => handle_stash_create(params, ctx).await,
        "stash_pop" => handle_stash_pop(params, ctx).await,
        "stash_apply" => handle_stash_apply(params, ctx).await,
        "stash_drop" => handle_stash_drop(params, ctx).await,
        "stash_count" => handle_stash_count(params, ctx).await,
        _ => Err(ExtensionError::method_not_found()),
    }
}

pub async fn handle_stash_list(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
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

    let output = run_git(ctx, &["stash", "list", "--format=%gd%x00%gs%x00%ci"]).await?;
    let mut items = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(3, '\0').collect();
        if parts.len() < 3 {
            continue;
        }
        let index_str = parts[0].trim_start_matches("stash@{").trim_end_matches('}');
        let index: u32 = index_str.parse().unwrap_or(0);
        items.push(GitStashEntry {
            index,
            message: parts[1].to_string(),
            date: parts[2].to_string(),
            branch: String::new(),
        });
    }

    let total = items.len();
    let end = (skip + limit).min(total);
    let page = items[skip..end].to_vec();
    let has_more = end < total;
    let next_cursor = if has_more {
        encode_cursor_offset(end)
    } else {
        None
    };

    Ok(serde_json::json!({
        "items": page,
        "nextCursor": next_cursor,
        "hasMore": has_more,
    }))
}

pub async fn handle_stash_create(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let message: Option<String> = optional_param_str(&params, "message");
    let include_untracked: bool = params
        .get("includeUntracked")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let keep_index: bool = params
        .get("keepIndex")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let status = run_git(ctx, &["status", "--porcelain"]).await?;
    if status.trim().is_empty() {
        return Err(ExtensionError::invalid_params("no local changes to save"));
    }

    let mut args = vec!["stash", "push"];
    if let Some(ref msg) = message {
        args.push("-m");
        args.push(msg);
    }
    if include_untracked {
        args.push("--include-untracked");
    }
    if keep_index {
        args.push("--keep-index");
    }

    run_git(ctx, &args).await?;
    let list = run_git(ctx, &["stash", "list", "--format=%gd%x00%gs", "-n", "1"]).await?;
    let (index, msg_out) = if let Some(first_line) = list.lines().next() {
        let parts: Vec<&str> = first_line.splitn(2, '\0').collect();
        let idx_str = parts[0].trim_start_matches("stash@{").trim_end_matches('}');
        let idx: u32 = idx_str.parse().unwrap_or(0);
        let m = parts.get(1).map(|s| s.to_string()).unwrap_or_default();
        (idx, m)
    } else {
        (0u32, String::new())
    };

    Ok(serde_json::json!({
        "index": index,
        "message": msg_out,
        "created": true,
    }))
}

pub async fn handle_stash_pop(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let index: u32 = require_param(&params, "index")?;
    let stash_ref = format!("stash@{{{index}}}");
    match run_git(ctx, &["stash", "pop", &stash_ref]).await {
        Ok(_) => Ok(serde_json::json!({"index": index, "popped": true})),
        Err(e) => {
            if matches!(e.code, -32603) {
                if let Some(Value::String(ref msg)) = e.data {
                    if msg.contains("CONFLICT") || msg.contains("conflict") {
                        return Err(ExtensionError::invalid_params(
                            "stash pop resulted in merge conflict",
                        ));
                    }
                }
                Err(ExtensionError::not_found(format!(
                    "stash entry {index} not found"
                )))
            } else {
                Err(e)
            }
        }
    }
}

pub async fn handle_stash_apply(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let index: u32 = require_param(&params, "index")?;
    let stash_ref = format!("stash@{{{index}}}");
    match run_git(ctx, &["stash", "apply", &stash_ref]).await {
        Ok(_) => Ok(serde_json::json!({"index": index, "applied": true})),
        Err(e) => {
            if matches!(e.code, -32603) {
                if let Some(Value::String(ref msg)) = e.data {
                    if msg.contains("CONFLICT") || msg.contains("conflict") {
                        return Err(ExtensionError::invalid_params(
                            "stash apply resulted in merge conflict",
                        ));
                    }
                }
                Err(ExtensionError::not_found(format!(
                    "stash entry {index} not found"
                )))
            } else {
                Err(e)
            }
        }
    }
}

pub async fn handle_stash_drop(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:stage")?;
    let index: u32 = require_param(&params, "index")?;
    let stash_ref = format!("stash@{{{index}}}");
    run_git(ctx, &["stash", "drop", &stash_ref]).await?;
    Ok(serde_json::json!({"index": index, "dropped": true}))
}

pub async fn handle_stash_count(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let _ = params;
    let list = run_git(ctx, &["stash", "list", "--format=%gd"]).await?;
    let count = list.lines().filter(|l| !l.is_empty()).count();

    if count == 0 {
        return Ok(serde_json::json!({
            "count": 0,
            "files": serde_json::json!([]),
        }));
    }

    let stat = run_git(ctx, &["stash", "show", "--stat", "--format=", "stash@{0}"])
        .await
        .unwrap_or_default();
    let mut files = Vec::new();
    for line in stat.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.contains("file")
            || trimmed.contains("insertion")
            || trimmed.contains("deletion")
        {
            continue;
        }
        let parts: Vec<&str> = trimmed.split('|').collect();
        if parts.len() >= 2 {
            files.push(serde_json::json!({
                "path": parts[0].trim(),
                "insertions": 0,
                "deletions": 0,
            }));
        }
    }

    Ok(serde_json::json!({
        "count": count,
        "files": files,
    }))
}
