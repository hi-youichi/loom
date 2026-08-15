use serde_json::Value;

use super::*;
use super::{ExtensionContext, ExtensionError};

pub async fn handle_diff(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let staged: bool = params
        .get("staged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let path: Option<String> = optional_param_str(&params, "path");
    let unified: u32 = params.get("unified").and_then(|v| v.as_u64()).unwrap_or(3) as u32;

    let unified_str = unified.to_string();
    let unified_arg = format!("--unified={unified_str}");
    let mut args: Vec<&str> = vec!["diff", &unified_arg];
    if staged {
        args.push("--cached");
    }
    args.push("--no-color");
    if let Some(ref p) = path {
        args.push("--");
        args.push(p);
    }

    let diff_text = run_git(ctx, &args).await?;
    let stat_args: Vec<&str> = if staged {
        vec!["diff", "--stat", "--cached"]
    } else {
        vec!["diff", "--stat"]
    };
    let stat_text = run_git(ctx, &stat_args).await?;
    let summary = parse_diff_output(&diff_text, &stat_text);
    Ok(serde_json::to_value(summary).unwrap_or(Value::Null))
}

pub async fn handle_file_diff(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let file_path: String = require_param(&params, "filePath")?;
    let staged: bool = params
        .get("staged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let unified: u32 = params.get("unified").and_then(|v| v.as_u64()).unwrap_or(3) as u32;

    let unified_str = unified.to_string();
    let unified_arg = format!("--unified={unified_str}");
    let mut args: Vec<&str> = vec!["diff", &unified_arg, "--no-color"];
    if staged {
        args.push("--cached");
    }
    args.push("--");
    args.push(&file_path);

    let diff_text = run_git(ctx, &args).await?;

    let original = if staged {
        let show_arg = format!(":{file_path}");
        run_git(ctx, &["show", &show_arg]).await.unwrap_or_default()
    } else {
        let show_arg = format!("HEAD:{file_path}");
        run_git(ctx, &["show", &show_arg]).await.unwrap_or_default()
    };

    let modified_path = ctx.working_directory.as_ref().map(|d| d.join(&file_path));
    let modified = match &modified_path {
        Some(p) if p.exists() => std::fs::read_to_string(p).unwrap_or_default(),
        _ => String::new(),
    };

    let stat_text = run_git(ctx, &["diff", "--stat", "--", &file_path])
        .await
        .unwrap_or_default();
    let summary = parse_diff_output(&diff_text, &stat_text);

    Ok(serde_json::json!({
        "filePath": file_path,
        "originalContent": original,
        "modifiedContent": modified,
        "hunks": summary.hunks,
    }))
}

pub async fn handle_commit_files(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let commit_sha: String = require_param(&params, "commitSha")?;

    let output = run_git(ctx, &["show", "--stat", "--name-status", &commit_sha]).await?;
    let numstat_output = run_git(ctx, &["show", "--numstat", "--format=", &commit_sha]).await?;

    let mut numstat_map: std::collections::HashMap<String, (u32, u32)> =
        std::collections::HashMap::new();
    for line in numstat_output.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() >= 3 {
            let ins: u32 = parts[0].parse().unwrap_or(0);
            let del: u32 = parts[1].parse().unwrap_or(0);
            let path = if parts[2].contains(" => ") {
                let rename_parts: Vec<&str> = parts[2].rsplitn(2, " => ").collect();
                rename_parts.first().unwrap_or(&parts[2]).to_string()
            } else {
                parts[2].to_string()
            };
            numstat_map.insert(path, (ins, del));
        }
    }

    let mut files = Vec::new();
    let mut total_insertions = 0u32;
    let mut total_deletions = 0u32;

    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() == 2 {
            let status_char = parts[0].chars().next().unwrap_or('M');
            let status = match status_char {
                'A' => "added",
                'D' => "deleted",
                'R' => "renamed",
                'C' => "copied",
                _ => "modified",
            };
            let path = if parts[0].starts_with('R') || parts[0].starts_with('C') {
                let rename_parts: Vec<&str> = parts[1].splitn(2, '\t').collect();
                rename_parts.last().unwrap_or(&parts[1]).to_string()
            } else {
                parts[1].to_string()
            };
            let (insertions, deletions) = numstat_map.get(&path).copied().unwrap_or((0, 0));
            total_insertions += insertions;
            total_deletions += deletions;
            files.push(serde_json::json!({
                "path": path,
                "status": status,
                "insertions": insertions,
                "deletions": deletions,
            }));
        }
    }

    Ok(serde_json::json!({
        "commitSha": commit_sha,
        "files": files,
        "totalInsertions": total_insertions,
        "totalDeletions": total_deletions,
    }))
}

pub async fn handle_commit_file_diff(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let commit_sha: String = require_param(&params, "commitSha")?;
    let file_path: String = require_param(&params, "filePath")?;
    let unified: u32 = params.get("unified").and_then(|v| v.as_u64()).unwrap_or(3) as u32;
    let unified_str = unified.to_string();
    let unified_arg = format!("--unified={unified_str}");

    let diff_text = run_git(ctx, &["show", &commit_sha, &unified_arg, "--", &file_path]).await?;
    let stat_text = run_git(ctx, &["show", &commit_sha, "--stat", "--", &file_path]).await?;
    let summary = parse_diff_output(&diff_text, &stat_text);

    Ok(serde_json::json!({
        "commitSha": commit_sha,
        "filePath": file_path,
        "hunks": summary.hunks,
        "stat": summary.stat,
    }))
}
