use serde_json::Value;

use super::*;
use super::{ExtensionContext, ExtensionError};

pub async fn handle(
    sub: &str,
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    match sub {
        "list" => handle_identity_list(params, ctx).await,
        "get" => handle_identity_get(params, ctx).await,
        "get_global" => handle_identity_get_global(params, ctx).await,
        "create" => handle_identity_create(params, ctx).await,
        "update" => handle_identity_update(params, ctx).await,
        "delete" => handle_identity_delete(params, ctx).await,
        "set" => handle_identity_set(params, ctx).await,
        "discover_credentials" => handle_identity_discover_credentials(params, ctx).await,
        _ => Err(ExtensionError::method_not_found()),
    }
}

fn profiles_path() -> std::path::PathBuf {
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        std::path::Path::new(&home)
            .join(".loom")
            .join("identity-profiles.json")
    } else {
        std::path::PathBuf::from(".loom/identity-profiles.json")
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct IdentityProfile {
    profile_id: String,
    name: String,
    email: String,
}

fn load_profiles() -> Vec<IdentityProfile> {
    let path = profiles_path();
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&content).unwrap_or_default()
}

fn save_profiles(profiles: &[IdentityProfile]) -> Result<(), ExtensionError> {
    let path = profiles_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let content = serde_json::to_string_pretty(profiles).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(e.to_string())),
    })?;
    std::fs::write(&path, content).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(e.to_string())),
    })?;
    Ok(())
}

pub async fn handle_identity_list(
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

    let stored = load_profiles();
    let mut items: Vec<GitIdentity> = stored
        .into_iter()
        .map(|p| GitIdentity {
            profile_id: p.profile_id,
            name: p.name,
            email: p.email,
            scope: IdentityScope::Global,
        })
        .collect();

    let repo_name = run_git(ctx, &["config", "user.name"])
        .await
        .ok()
        .map(|s| s.trim().to_string());
    let repo_email = run_git(ctx, &["config", "user.email"])
        .await
        .ok()
        .map(|s| s.trim().to_string());
    if let (Some(name), Some(email)) = (repo_name, repo_email) {
        if !name.is_empty() && !email.is_empty() {
            items.insert(
                0,
                GitIdentity {
                    profile_id: "__repo__".to_string(),
                    name,
                    email,
                    scope: IdentityScope::Repo,
                },
            );
        }
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

pub async fn handle_identity_get(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let path: Option<String> = optional_param_str(&params, "path");

    let name = run_git(ctx, &["config", "user.name"])
        .await
        .unwrap_or_default();
    let email = run_git(ctx, &["config", "user.email"])
        .await
        .unwrap_or_default();

    if name.trim().is_empty() && email.trim().is_empty() {
        return Err(ExtensionError::not_found(
            "no identity configured for this repository",
        ));
    }

    let source = if path.is_some() {
        "worktree_config"
    } else {
        "repo_config"
    };
    Ok(serde_json::json!({
        "profileId": "__active__",
        "name": name.trim(),
        "email": email.trim(),
        "source": source,
    }))
}

pub async fn handle_identity_get_global(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let _ = params;
    let name = run_git(ctx, &["config", "--global", "user.name"])
        .await
        .unwrap_or_default();
    let email = run_git(ctx, &["config", "--global", "user.email"])
        .await
        .unwrap_or_default();

    Ok(serde_json::json!({
        "name": if name.trim().is_empty() { None } else { Some(name.trim()) },
        "email": if email.trim().is_empty() { None } else { Some(email.trim()) },
        "source": "global_config",
    }))
}

pub async fn handle_identity_create(
    params: Value,
    _ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(_ctx, "git:identity")?;
    let profile_id: String = require_param(&params, "profileId")?;
    let name: String = require_param(&params, "name")?;
    let email: String = require_param(&params, "email")?;

    let mut profiles = load_profiles();
    if profiles.iter().any(|p| p.profile_id == profile_id) {
        return Err(ExtensionError {
            code: -32005,
            message: "conflict".into(),
            data: Some(Value::String(format!(
                "profile '{profile_id}' already exists"
            ))),
        });
    }
    profiles.push(IdentityProfile {
        profile_id: profile_id.clone(),
        name: name.clone(),
        email: email.clone(),
    });
    save_profiles(&profiles)?;

    Ok(serde_json::json!({
        "profileId": profile_id,
        "name": name,
        "email": email,
        "created": true,
    }))
}

pub async fn handle_identity_update(
    params: Value,
    _ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(_ctx, "git:identity")?;
    let profile_id: String = require_param(&params, "profileId")?;
    let name: Option<String> = optional_param_str(&params, "name");
    let email: Option<String> = optional_param_str(&params, "email");

    let mut profiles = load_profiles();
    let profile = profiles
        .iter_mut()
        .find(|p| p.profile_id == profile_id)
        .ok_or_else(|| ExtensionError::not_found(format!("profile '{profile_id}' not found")))?;

    if let Some(n) = name {
        profile.name = n;
    }
    if let Some(e) = email {
        profile.email = e;
    }
    let result_name = profile.name.clone();
    let result_email = profile.email.clone();
    save_profiles(&profiles)?;

    Ok(serde_json::json!({
        "profileId": profile_id,
        "name": result_name,
        "email": result_email,
        "updated": true,
    }))
}

pub async fn handle_identity_delete(
    params: Value,
    _ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(_ctx, "git:identity")?;
    let profile_id: String = require_param(&params, "profileId")?;

    let mut profiles = load_profiles();
    let initial_len = profiles.len();
    profiles.retain(|p| p.profile_id != profile_id);
    if profiles.len() == initial_len {
        return Err(ExtensionError::not_found(format!(
            "profile '{profile_id}' not found"
        )));
    }
    save_profiles(&profiles)?;

    Ok(serde_json::json!({"profileId": profile_id, "deleted": true}))
}

pub async fn handle_identity_set(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    require_git_scope(ctx, "git:identity")?;
    let profile_id: String = require_param(&params, "profileId")?;
    let path: Option<String> = optional_param_str(&params, "path");

    let profiles = load_profiles();
    let profile = profiles
        .iter()
        .find(|p| p.profile_id == profile_id)
        .ok_or_else(|| ExtensionError::not_found(format!("profile '{profile_id}' not found")))?;

    let scope_arg = if path.is_some() {
        "--worktree"
    } else {
        "--local"
    };
    run_git(ctx, &["config", scope_arg, "user.name", &profile.name]).await?;
    run_git(ctx, &["config", scope_arg, "user.email", &profile.email]).await?;

    let scope = if path.is_some() { "worktree" } else { "repo" };
    Ok(serde_json::json!({
        "profileId": profile_id,
        "path": path.unwrap_or_default(),
        "applied": true,
        "scope": scope,
    }))
}

pub async fn handle_identity_discover_credentials(
    params: Value,
    ctx: &ExtensionContext,
) -> Result<Value, ExtensionError> {
    let remote: String = require_param(&params, "remote")?;

    let url_output = run_git(ctx, &["remote", "get-url", &remote]).await;
    let url_raw = match url_output {
        Ok(u) => u,
        Err(_) => {
            return Err(ExtensionError::not_found(format!(
                "remote '{remote}' not found"
            )))
        }
    };

    let url = sanitize_remote_url(url_raw.trim());
    let url_type = classify_remote_url(&url);

    let mut helpers = Vec::new();

    match url_type {
        RemoteUrlType::Https => {
            helpers.push(serde_json::json!({
                "type": "credential_helper",
                "available": true,
                "helper": "credential-manager",
            }));
            let helper_config = run_git(ctx, &["config", "credential.helper"])
                .await
                .unwrap_or_default();
            if !helper_config.trim().is_empty() {
                helpers.push(serde_json::json!({
                    "type": "credential_helper",
                    "available": true,
                    "helper": helper_config.trim(),
                }));
            }
        }
        RemoteUrlType::Ssh => {
            if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
            {
                let ssh_dir = std::path::Path::new(&home).join(".ssh");
                for key_name in &["id_ed25519", "id_rsa", "id_ecdsa", "id_dsa"] {
                    let key_path = ssh_dir.join(key_name);
                    if key_path.exists() {
                        let key_type = match *key_name {
                            "id_ed25519" => "ed25519",
                            "id_rsa" => "rsa",
                            "id_ecdsa" => "ecdsa",
                            _ => "dsa",
                        };
                        helpers.push(serde_json::json!({
                            "type": "ssh_key",
                            "available": true,
                            "keyType": key_type,
                        }));
                    }
                }
            }
        }
        RemoteUrlType::File => {
            helpers.push(serde_json::json!({
                "type": "file",
                "available": true,
            }));
        }
    }

    let has_credentials = !helpers.is_empty();

    Ok(serde_json::json!({
        "remote": remote,
        "credentialHelpers": helpers,
        "hasCredentials": has_credentials,
    }))
}
