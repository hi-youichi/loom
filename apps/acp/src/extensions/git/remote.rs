use serde_json::Value;

use super::*;
use super::{ExtensionContext, ExtensionError};

pub async fn handle_remotes(
    _params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let repo_dir = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let remotes = anureo_git::facade::remotes(&repo_dir)
        .await
        .map_err(ext_err_from_git)?;
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

    let remote_name = remote.unwrap_or("origin".to_string());
    let repo = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    // §6.2: git2 native fetch first; auth failures fall back to `git fetch`
    // (GCM/ssh-agent) with a fetch_via annotation.
    let result = anureo_git::facade::fetch(&repo, &remote_name, branch.as_deref(), prune)
        .await
        .map_err(|e| match e.kind() {
            anureo_git::GitErrorKind::NotFound => ExtensionError::not_found(format!(
                "remote '{remote_name}' not found or unreachable"
            )),
            _ => ext_err_from_git(e),
        })?;

    let fetched_refs: Vec<serde_json::Value> = result
        .fetched_refs
        .iter()
        .map(|r| {
            serde_json::json!({
                "ref": r.ref_name,
                "oldSha": r.old_sha,
                "newSha": r.new_sha,
            })
        })
        .collect();

    let mut payload = serde_json::json!({
        "remote": remote_name,
        "updated": true,
        "fetchedRefs": fetched_refs,
    });
    if let Some(via) = result.fetch_via {
        payload["fetchVia"] = serde_json::json!(via);
    }
    Ok(payload)
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
    let repo = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let branch_name = match &branch {
        Some(b) => b.clone(),
        None => {
            let status = anureo_git::facade::status(&repo)
                .await
                .map_err(ext_err_from_git)?;
            status.branch
        }
    };

    // §6.2: git2 credential callbacks first; auth failures fall back to
    // `git push` (GCM/ssh-agent) with a push_via annotation.
    let result = anureo_git::facade::push(&repo, &remote_name, &branch_name, force, set_upstream)
        .await
        .map_err(|e| match e.kind() {
            anureo_git::GitErrorKind::Conflict if !force => ExtensionError::invalid_params(
                "non-fast-forward push rejected; use force=true to override",
            ),
            _ => ext_err_from_git(e),
        })?;

    let mut payload = serde_json::json!({
        "remote": remote_name,
        "branch": branch_name,
        "pushed": true,
        "remoteSha": result.remote_sha,
    });
    if let Some(via) = result.push_via {
        payload["pushVia"] = serde_json::json!(via);
    }
    Ok(payload)
}

pub async fn handle_pull(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:remote")?;
    let remote: Option<String> = optional_param_str(&params, "remote");
    let branch: Option<String> = optional_param_str(&params, "branch");

    let remote_name = remote.unwrap_or("origin".to_string());
    let branch_name = branch.unwrap_or_default();
    let repo = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let pull_result = anureo_git::facade::pull(
        &repo,
        &remote_name,
        if branch_name.is_empty() {
            None
        } else {
            Some(branch_name.as_str())
        },
    )
    .await;
    let result = match pull_result {
        Ok(r) => r,
        Err(e) => {
            let conflicts = anureo_git::facade::in_progress(&repo)
                .await
                .ok()
                .flatten()
                .map(|ip| ip.conflict_files)
                .unwrap_or_default();
            if !conflicts.is_empty() {
                return Err(ExtensionError {
                    code: -32602,
                    message: "invalid_params".into(),
                    data: Some(serde_json::json!({"conflicts": conflicts})),
                });
            }
            return Err(ext_err_from_git(e));
        }
    };

    Ok(serde_json::json!({
        "remote": remote_name,
        "branch": branch_name,
        "pulled": true,
        "updated": true,
        "fastForward": result.fast_forward,
        "mergeCommit": result.merge_commit,
    }))
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
