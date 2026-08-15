use serde_json::Value;

use super::*;
use super::{ExtensionContext, ExtensionError};

pub async fn handle_status(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let path = optional_param_str(&params, "path");
    let _ = path;

    let output = run_git(ctx, &["status", "--porcelain=v2", "--branch"]).await?;
    let status = parse_porcelain_status_v2(&output);
    if let Some(op) = detect_operation(ctx).await {
        if status.in_progress.is_none() {
            let conflict_files: Vec<String> = output
                .lines()
                .filter(|l| l.starts_with("u "))
                .filter_map(|l| l.split_whitespace().last().map(|s| s.to_string()))
                .collect();
            if !conflict_files.is_empty() {
                return Ok(serde_json::to_value(GitStatus {
                    in_progress: Some(GitInProgress {
                        operation: op,
                        conflict_files,
                    }),
                    ..status
                })
                .unwrap_or(Value::Null));
            }
        }
    }
    Ok(serde_json::to_value(status).unwrap_or(Value::Null))
}

pub async fn handle_log(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let limit: Option<usize> = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let cursor: Option<String> = params
        .get("cursor")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let branch: Option<String> = optional_param_str(&params, "branch");
    let file_path: Option<String> = optional_param_str(&params, "filePath");

    let limit = limit.unwrap_or(30);
    let skip = decode_cursor_offset(&cursor);

    let format_arg =
        "--format=%H%x00%P%x00%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI%x00%s%x00%D%x01".to_string();
    let limit_str = limit.to_string();
    let skip_str = skip.to_string();
    let mut args: Vec<&str> = vec!["log", &format_arg, "-n", &limit_str, "--skip", &skip_str];

    if let Some(ref branch) = branch {
        args.push(branch);
    }
    if let Some(ref fp) = file_path {
        args.push("--");
        args.push(fp);
    }

    let output = run_git(ctx, &args).await?;

    let mut items = Vec::new();
    for entry in output.split('\x01') {
        let entry = entry.trim_start_matches('\n');
        if entry.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = entry.splitn(10, '\0').collect();
        if parts.len() < 10 {
            continue;
        }
        items.push(GitCommitInfo {
            sha: parts[0].to_string(),
            parents: parts[1].split_whitespace().map(|s| s.to_string()).collect(),
            author: parts[2].to_string(),
            author_email: parts[3].to_string(),
            author_date: parts[4].to_string(),
            committer: parts[5].to_string(),
            committer_email: parts[6].to_string(),
            committer_date: parts[7].to_string(),
            message: parts[8].trim_end().to_string(),
            refs: parts[9]
                .split(", ")
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect(),
        });
    }

    let has_more = items.len() >= limit;
    let next_cursor = if has_more {
        encode_cursor_offset(skip + items.len())
    } else {
        None
    };

    Ok(serde_json::json!({
        "items": items,
        "nextCursor": next_cursor,
        "hasMore": has_more,
    }))
}
