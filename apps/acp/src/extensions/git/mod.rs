pub mod branches;
pub mod diff;
pub mod identity;
pub mod merge_rebase;
pub mod remote;
pub mod staging;
pub mod stash;
pub mod status;
pub mod worktree;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

pub(crate) use super::{ExtensionContext, ExtensionError, ExtensionHandler};

pub struct GitHandler {
    global_bus: Option<std::sync::Arc<crate::global_events::GlobalEventBus>>,
}

impl Default for GitHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHandler {
    pub fn new() -> Self {
        Self { global_bus: None }
    }

    /// Broadcast `git.updated` on the global event bus after successful writes.
    pub fn with_global_bus(
        mut self,
        bus: std::sync::Arc<crate::global_events::GlobalEventBus>,
    ) -> Self {
        self.global_bus = Some(bus);
        self
    }
    /// Mutating methods that should broadcast `git.updated` on success.
    const WRITE_METHODS: &'static [&'static str] = &[
        "stage_file",
        "stage_files",
        "unstage_file",
        "unstage_files",
        "stage_hunk",
        "unstage_hunk",
        "revert_file",
        "revert_hunk",
        "checkout_branch",
        "create_branch",
        "rename_branch",
        "delete_branch",
        "delete_remote_branch",
        "remove_remote",
        "commit",
        "push",
        "pull",
        "merge",
        "merge_abort",
        "merge_continue",
        "rebase",
        "rebase_abort",
        "rebase_continue",
        "cherry_pick",
        "reset",
        "clean",
        "stash/create",
        "stash/pop",
        "stash/apply",
        "stash/drop",
    ];

    fn publish_git_updated(&self, ctx: &ExtensionContext) {
        if let Some(bus) = &self.global_bus {
            bus.publish(
                "git",
                "git.updated",
                serde_json::json!({
                    "directory": ctx.working_directory
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                }),
            );
        }
    }
}

#[async_trait]
impl ExtensionHandler for GitHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        if let Some(sub) = method.strip_prefix("identity/") {
            return identity::handle(sub, params, ctx).await;
        }
        let is_write = Self::WRITE_METHODS.contains(&method)
            || method
                .strip_prefix("stash/")
                .is_some_and(|sub| matches!(sub, "create" | "pop" | "apply" | "drop"));
        if let Some(sub) = method.strip_prefix("stash/") {
            let flat = match sub {
                "list" => "stash_list",
                "create" => "stash_create",
                "pop" => "stash_pop",
                "apply" => "stash_apply",
                "drop" => "stash_drop",
                "count" => "stash_count",
                _ => sub,
            };
            return stash::handle(params, flat, ctx).await;
        }
        match method {
            "check" => status::handle_check(params, ctx).await,
            "status" => status::handle_status(params, ctx).await,
            "diff" => diff::handle_diff(params, ctx).await,
            "file_diff" => diff::handle_file_diff(params, ctx).await,
            "log" => status::handle_log(params, ctx).await,
            "commit_files" => diff::handle_commit_files(params, ctx).await,
            "commit_file_diff" => diff::handle_commit_file_diff(params, ctx).await,
            "stage_file" => staging::handle_stage_file(params, ctx).await,
            "stage_files" => staging::handle_stage_files(params, ctx).await,
            "unstage_file" => staging::handle_unstage_file(params, ctx).await,
            "unstage_files" => staging::handle_unstage_files(params, ctx).await,
            "stage_hunk" => staging::handle_stage_hunk(params, ctx).await,
            "unstage_hunk" => staging::handle_unstage_hunk(params, ctx).await,
            "revert_file" => staging::handle_revert_file(params, ctx).await,
            "revert_hunk" => staging::handle_revert_hunk(params, ctx).await,
            "branches" => branches::handle_branches(params, ctx).await,
            "checkout_branch" => branches::handle_checkout_branch(params, ctx).await,
            "create_branch" => branches::handle_create_branch(params, ctx).await,
            "rename_branch" => branches::handle_rename_branch(params, ctx).await,
            "delete_branch" => branches::handle_delete_branch(params, ctx).await,
            "delete_remote_branch" => remote::handle_delete_remote_branch(params, ctx).await,
            "remotes" => remote::handle_remotes(params, ctx).await,
            "remote_url" => remote::handle_remote_url(params, ctx).await,
            "remove_remote" => remote::handle_remove_remote(params, ctx).await,
            "fetch" => remote::handle_fetch(params, ctx).await,
            "commit" => staging::handle_commit(params, ctx).await,
            "generate_commit_message" => Err(ExtensionError {
                code: -32603,
                message: "internal_error".into(),
                data: Some(Value::String(
                    "generate_commit_message not yet implemented — requires small-model integration".into(),
                )),
            }),
            "generate_pr_description" => Err(ExtensionError {
                code: -32603,
                message: "internal_error".into(),
                data: Some(Value::String(
                    "generate_pr_description not yet implemented — requires small-model integration".into(),
                )),
            }),
            "push" => remote::handle_push(params, ctx).await,
            "pull" => remote::handle_pull(params, ctx).await,
            "merge" => merge_rebase::handle_merge(params, ctx).await,
            "merge_abort" => merge_rebase::handle_merge_abort(params, ctx).await,
            "merge_continue" => merge_rebase::handle_merge_continue(params, ctx).await,
            "rebase" => merge_rebase::handle_rebase(params, ctx).await,
            "rebase_abort" => merge_rebase::handle_rebase_abort(params, ctx).await,
            "rebase_continue" => merge_rebase::handle_rebase_continue(params, ctx).await,
            "conflict_details" => merge_rebase::handle_conflict_details(params, ctx).await,
            "checkout_commit" => branches::handle_checkout_commit(params, ctx).await,
            "cherry_pick" => merge_rebase::handle_cherry_pick(params, ctx).await,
            "revert_commit" => merge_rebase::handle_revert_commit(params, ctx).await,
            "reset_to_commit" => merge_rebase::handle_reset_to_commit(params, ctx).await,
            "validate_worktree_directory" => worktree::handle_validate_worktree_directory(params, ctx).await,
            "canonicalize_worktree_state" => worktree::handle_canonicalize_worktree_state(params, ctx).await,
            "is_linked_worktree" => worktree::handle_is_linked_worktree(params, ctx).await,
            _ => Err(ExtensionError::method_not_found()),
        }
        .inspect(|_| {
            if is_write {
                self.publish_git_updated(ctx);
            }
        })
    }

    fn capabilities(&self) -> Value {
        let mut caps = serde_json::Map::new();
        for method in [
            "check",
            "status",
            "diff",
            "file_diff",
            "log",
            "commit_files",
            "commit_file_diff",
            "stage_file",
            "stage_files",
            "unstage_file",
            "unstage_files",
            "stage_hunk",
            "unstage_hunk",
            "revert_file",
            "revert_hunk",
            "branches",
            "checkout_branch",
            "create_branch",
            "rename_branch",
            "delete_branch",
            "delete_remote_branch",
            "remotes",
            "remote_url",
            "remove_remote",
            "fetch",
            "commit",
            "generate_commit_message",
            "generate_pr_description",
            "push",
            "pull",
            "stash_list",
            "stash_create",
            "stash_pop",
            "stash_apply",
            "stash_drop",
            "stash_count",
            "merge",
            "merge_abort",
            "merge_continue",
            "rebase",
            "rebase_abort",
            "rebase_continue",
            "conflict_details",
            "checkout_commit",
            "cherry_pick",
            "revert_commit",
            "reset_to_commit",
            "validate_worktree_directory",
            "canonicalize_worktree_state",
            "is_linked_worktree",
            "identity_list",
            "identity_get",
            "identity_get_global",
            "identity_create",
            "identity_update",
            "identity_delete",
            "identity_set",
            "identity_discover_credentials",
        ] {
            caps.insert(method.to_string(), Value::Bool(true));
        }
        Value::Object(caps)
    }
}

// ── Shared types and helpers ───────────────────────────────────────────
// Typed structs live in foundation/git (`anureo_git::types`), matching this
// extension's JSON contract byte-for-byte; re-exported so handlers keep
// `use super::*` imports unchanged.

pub use anureo_git::types::{
    classify_remote_url, sanitize_remote_url, ConflictFile, ConflictHunk, ConflictLine,
    ConflictLineKind, FetchedRef, GitBranch, GitCommitInfo, GitDiffHunk, GitDiffLine,
    GitDiffLineKind, GitDiffStat, GitDiffSummary, GitFileStatus, GitIdentity, GitInProgress,
    GitOperation, GitRemote, GitStashEntry, GitStatus, GitStatusFile, IdentityScope, RemoteUrlType,
};

pub(crate) use anureo_git::cli::parsers::{parse_diff_output, parse_porcelain_status_v2};

pub(crate) fn ext_err_from_git(e: anureo_git::GitError) -> ExtensionError {
    use anureo_git::GitErrorKind as K;
    match e.kind() {
        K::NotFound => ExtensionError::not_found(e.message().to_string()),
        K::InvalidParams => ExtensionError::invalid_params(e.message().to_string()),
        K::Conflict => ExtensionError::conflict(e.message().to_string()),
        K::Forbidden => ExtensionError::forbidden(e.message().to_string()),
        _ => ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(e.data())),
        },
    }
}

// ── Git CLI helpers ────────────────────────────────────────────────────

pub(crate) fn require_git_scope(ctx: &ExtensionContext, scope: &str) -> Result<(), ExtensionError> {
    super::auth::check_server_policy(ctx, "git", scope)
}

pub(crate) async fn run_git_apply(
    ctx: &ExtensionContext,
    args: &[&str],
    patch: &str,
) -> Result<(), ExtensionError> {
    anureo_git::facade::run_apply_raw(ctx.working_directory.as_deref(), args, patch)
        .await
        .map_err(ext_err_from_git)
}

pub(crate) async fn run_git(
    ctx: &ExtensionContext,
    args: &[&str],
) -> Result<String, ExtensionError> {
    anureo_git::facade::run_raw(ctx.working_directory.as_deref(), args)
        .await
        .map_err(ext_err_from_git)
}

pub(crate) fn require_param<T: for<'de> Deserialize<'de>>(
    params: &Value,
    key: &str,
) -> Result<T, ExtensionError> {
    let val = params.get(key).ok_or_else(|| {
        ExtensionError::invalid_params(format!("missing required parameter: {key}"))
    })?;
    serde_json::from_value(val.clone())
        .map_err(|_| ExtensionError::invalid_params(format!("invalid type for parameter: {key}")))
}

pub(crate) fn optional_param_str(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .filter(|v| !v.is_null())
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

pub(crate) fn validate_git_path(path: &str, _ctx: &ExtensionContext) -> Result<(), ExtensionError> {
    if path.is_empty() {
        return Err(ExtensionError::invalid_params("path is empty"));
    }
    if path.starts_with('/') || path.starts_with('\\') || path.contains("..") {
        return Err(ExtensionError::invalid_params(format!(
            "path outside worktree: {path}"
        )));
    }
    Ok(())
}

pub(crate) fn encode_cursor_offset(offset: usize) -> Option<String> {
    use super::pagination::encode_cursor;
    if offset == 0 {
        return None;
    }
    Some(encode_cursor(serde_json::json!({"offset": offset})))
}

pub(crate) fn decode_cursor_offset(cursor: &Option<String>) -> usize {
    match cursor {
        None => 0,
        Some(raw) => {
            if raw.is_empty() {
                return 0;
            }
            match hex_decode_safe(raw) {
                Some(bytes) => {
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        v.get("offset")
                            .and_then(|o| o.as_u64())
                            .map(|o| o as usize)
                            .unwrap_or(0)
                    } else {
                        0
                    }
                }
                None => 0,
            }
        }
    }
}

fn hex_decode_safe(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

pub(crate) async fn resolve_git_dir(ctx: &ExtensionContext) -> Option<std::path::PathBuf> {
    match run_git(ctx, &["rev-parse", "--absolute-git-dir"]).await {
        Ok(dir) => Some(std::path::PathBuf::from(dir.trim())),
        Err(_) => {
            if let Some(dir) = &ctx.working_directory {
                let git_dir = dir.join(".git");
                if git_dir.exists() {
                    Some(git_dir)
                } else {
                    None
                }
            } else {
                None
            }
        }
    }
}

pub(crate) async fn check_in_progress(ctx: &ExtensionContext, marker_files: &[&str]) -> bool {
    if let Some(git_dir) = resolve_git_dir(ctx).await {
        marker_files.iter().any(|f| git_dir.join(f).exists())
    } else {
        false
    }
}

pub(crate) async fn detect_operation(ctx: &ExtensionContext) -> Option<GitOperation> {
    if check_in_progress(ctx, &["MERGE_HEAD"]).await {
        Some(GitOperation::Merge)
    } else if check_in_progress(ctx, &["rebase-merge", "rebase-apply"]).await {
        Some(GitOperation::Rebase)
    } else if check_in_progress(ctx, &["CHERRY_PICK_HEAD"]).await {
        Some(GitOperation::CherryPick)
    } else if check_in_progress(ctx, &["REVERT_HEAD"]).await {
        Some(GitOperation::Revert)
    } else {
        None
    }
}
