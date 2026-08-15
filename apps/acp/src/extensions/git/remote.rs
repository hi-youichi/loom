use serde_json::Value;

use super::*;
use super::{ExtensionContext, ExtensionError};

pub async fn handle_remotes(
    _params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let output = run_git(ctx, &["remote", "-v"]).await?;
    let mut seen = std::collections::HashSet::new();
    let mut remotes = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let name = parts[0].to_string();
        let raw_url = parts[1].to_string();
        if seen.contains(&name) {
            continue;
        }
        seen.insert(name.clone());
        let url = sanitize_remote_url(&raw_url);
        let url_type = classify_remote_url(&url);
        remotes.push(GitRemote {
            name,
            url,
            url_type,
        });
    }
    Ok(serde_json::json!({"remotes": remotes}))
}

pub async fn handle_remote_url(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let remote: String = require_param(&params, "remote")?;
    let output = run_git(ctx, &["remote", "get-url", &remote]).await;
    match output {
        Ok(url_raw) => {
            let url = sanitize_remote_url(url_raw.trim());
            let url_type = classify_remote_url(&url);
            Ok(serde_json::json!({
                "remote": remote,
                "url": url,
                "urlType": url_type,
            }))
        }
        Err(_) => Err(ExtensionError::not_found(format!(
            "remote '{remote}' not found"
        ))),
    }
}

pub async fn handle_remove_remote(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:remote")?;
    let remote: String = require_param(&params, "remote")?;
    run_git(ctx, &["remote", "remove", &remote]).await?;
    Ok(serde_json::json!({"remote": remote, "removed": true}))
}

pub async fn handle_fetch(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:remote")?;
    let remote: Option<String> = optional_param_str(&params, "remote");
    let branch: Option<String> = optional_param_str(&params, "branch");
    let prune: bool = params
        .get("prune")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut args = vec!["fetch"];
    let remote_name = remote.unwrap_or("origin".to_string());
    args.push(&remote_name);
    if let Some(ref b) = branch {
        args.push(b);
    }
    if prune {
        args.push("--prune");
    }

    match run_git(ctx, &args).await {
        Ok(output) => {
            let fetched_refs = parse_fetch_refs(&output, &remote_name, ctx).await;
            Ok(serde_json::json!({
                "remote": remote_name,
                "updated": true,
                "fetchedRefs": fetched_refs,
            }))
        }
        Err(e) => {
            if matches!(e.code, -32603) {
                Err(ExtensionError::not_found(format!(
                    "remote '{remote_name}' not found or unreachable"
                )))
            } else {
                Err(e)
            }
        }
    }
}

async fn parse_fetch_refs(
    output: &str,
    remote_name: &str,
    ctx: &ExtensionContext,
) -> Vec<FetchedRef> {
    let mut refs = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.contains(" -> ") {
            let arrow_pos = trimmed.rfind(" -> ").unwrap_or(trimmed.len());
            let before_arrow = trimmed[..arrow_pos].trim();
            let after_arrow = trimmed[arrow_pos + 4..].trim();
            let ref_name = format!("{remote_name}/{after_arrow}");

            let (old_sha, new_sha) = if let Some(dots) = before_arrow.find("..") {
                let old = before_arrow[..dots].trim().to_string();
                let new = before_arrow[dots + 2..].trim().to_string();
                (old, new)
            } else {
                let sha = run_git(ctx, &["rev-parse", &ref_name])
                    .await
                    .unwrap_or_default();
                (String::new(), sha.trim().to_string())
            };

            refs.push(FetchedRef {
                ref_name,
                old_sha,
                new_sha,
            });
        }
    }
    refs
}

pub async fn handle_push(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:remote")?;
    let remote: Option<String> = optional_param_str(&params, "remote");
    let branch: Option<String> = optional_param_str(&params, "branch");
    let force: bool = params
        .get("force")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let set_upstream: bool = params
        .get("setUpstream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let remote_name = remote.unwrap_or("origin".to_string());
    let branch_name = match &branch {
        Some(b) => b.clone(),
        None => run_git(ctx, &["rev-parse", "--abbrev-ref", "HEAD"])
            .await?
            .trim()
            .to_string(),
    };

    let mut args = vec!["push"];
    if set_upstream {
        args.push("-u");
    }
    if force {
        args.push("--force-with-lease");
    }
    args.push(&remote_name);
    args.push(&branch_name);

    match run_git(ctx, &args).await {
        Ok(_) => {
            let remote_ref = format!("{remote_name}/{branch_name}");
            let remote_sha = run_git(ctx, &["rev-parse", &remote_ref])
                .await
                .unwrap_or_default();
            Ok(serde_json::json!({
                "remote": remote_name,
                "branch": branch_name,
                "pushed": true,
                "remoteSha": remote_sha.trim(),
            }))
        }
        Err(e) => {
            if matches!(e.code, -32603) {
                if !force {
                    Err(ExtensionError::invalid_params(
                        "non-fast-forward push rejected; use force=true to override",
                    ))
                } else {
                    Err(e)
                }
            } else {
                Err(e)
            }
        }
    }
}

pub async fn handle_pull(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:remote")?;
    let remote: Option<String> = optional_param_str(&params, "remote");
    let branch: Option<String> = optional_param_str(&params, "branch");

    let remote_name = remote.unwrap_or("origin".to_string());
    let branch_name = branch.unwrap_or_default();

    let mut args = vec!["pull", &remote_name];
    if !branch_name.is_empty() {
        args.push(&branch_name);
    }

    let output = run_git(ctx, &args).await;
    match output {
        Ok(out) => {
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
                "remote": remote_name,
                "branch": branch_name,
                "pulled": true,
                "updated": true,
                "fastForward": fast_forward,
                "mergeCommit": merge_commit,
            }))
        }
        Err(e) => {
            if matches!(e.code, -32603) {
                if let Some(Value::String(ref msg)) = e.data {
                    if msg.contains("CONFLICT") || msg.contains("conflict") {
                        return Err(ExtensionError {
                            code: -32602,
                            message: "invalid_params".into(),
                            data: Some(
                                serde_json::json!({"conflicts": get_conflict_files(ctx).await}),
                            ),
                        });
                    }
                }
                Err(e)
            } else {
                Err(e)
            }
        }
    }
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

pub async fn handle_delete_remote_branch(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:remote")?;
    let remote: String = require_param(&params, "remote")?;
    let branch: String = require_param(&params, "branch")?;
    match run_git(ctx, &["push", &remote, "--delete", &branch]).await {
        Ok(_) => Ok(serde_json::json!({"remote": remote, "branch": branch, "deleted": true})),
        Err(e) => Err(e),
    }
}
