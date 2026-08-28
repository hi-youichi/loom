use serde_json::Value;

use super::*;
use super::{ExtensionContext, ExtensionError};

/// Lightweight repository probe used by project/worktree discovery.
///
/// Unlike `status`, this never enumerates changed files. A directory that is
/// not inside a repository is an expected negative result, not a protocol
/// error; execution failures unrelated to repository absence remain errors.
pub async fn handle_check(_params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    match anureo_git::facade::run_raw(
        ctx.working_directory.as_deref(),
        &["rev-parse", "--is-inside-work-tree"],
    )
    .await
    {
        Ok(output) => Ok(serde_json::json!({
            "isGitRepository": output.trim().eq_ignore_ascii_case("true"),
        })),
        Err(error) if error.kind() == anureo_git::GitErrorKind::NotFound => {
            Ok(serde_json::json!({ "isGitRepository": false }))
        }
        Err(error) => Err(ext_err_from_git(error)),
    }
}

pub async fn handle_status(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let path = optional_param_str(&params, "path");
    let _ = path;

    let repo_dir = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let status = anureo_git::facade::status(&repo_dir)
        .await
        .map_err(ext_err_from_git)?;
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

    let repo_dir = ctx
        .working_directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let query = anureo_git::backend::LogQuery {
        limit,
        skip,
        branch: branch.clone(),
        file_path: file_path.clone(),
    };
    let items = anureo_git::facade::log(&repo_dir, &query)
        .await
        .map_err(ext_err_from_git)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_capabilities::ClientCapabilitiesInfo;
    use std::process::Command;
    use tempfile::TempDir;

    fn context(directory: &std::path::Path) -> ExtensionContext {
        ExtensionContext {
            session_id: Some("test-session".into()),
            principal: "test-user".into(),
            connection_id: "test-connection".into(),
            working_directory: Some(directory.to_path_buf()),
            client_capabilities: ClientCapabilitiesInfo::default(),
        }
    }

    #[tokio::test]
    async fn check_returns_false_without_enumerating_status_for_non_repo() {
        let directory = TempDir::new().expect("temp directory");
        let result = handle_check(Value::Null, &context(directory.path()))
            .await
            .expect("negative probe should be a successful response");

        assert_eq!(result, serde_json::json!({ "isGitRepository": false }));
    }

    #[tokio::test]
    async fn check_returns_true_for_repo() {
        let directory = TempDir::new().expect("temp directory");
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(directory.path())
            .status()
            .expect("git should run");
        assert!(status.success());

        let result = handle_check(Value::Null, &context(directory.path()))
            .await
            .expect("repository probe should succeed");

        assert_eq!(result, serde_json::json!({ "isGitRepository": true }));
    }
}
