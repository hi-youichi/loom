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
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::Command;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitFileStatus {
    Unmodified,
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    Unmerged,
    Untracked,
    Ignored,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitOperation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusFile {
    pub path: String,
    pub index_status: GitFileStatus,
    pub working_status: GitFileStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInProgress {
    pub operation: GitOperation,
    pub conflict_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<GitStatusFile>,
    pub in_progress: Option<GitInProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitDiffLineKind {
    Context,
    Addition,
    Deletion,
    NoNewline,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffLine {
    pub kind: GitDiffLineKind,
    pub content: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffHunk {
    pub old_path: String,
    pub new_path: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
    pub lines: Vec<GitDiffLine>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffStat {
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffSummary {
    pub hunks: Vec<GitDiffHunk>,
    pub stat: GitDiffStat,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitInfo {
    pub sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub author_email: String,
    pub author_date: String,
    pub committer: String,
    pub committer_email: String,
    pub committer_date: String,
    pub message: String,
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranch {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub last_commit_sha: String,
    pub last_commit_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteUrlType {
    Https,
    Ssh,
    File,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRemote {
    pub name: String,
    pub url: String,
    pub url_type: RemoteUrlType,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStashEntry {
    pub index: u32,
    pub message: String,
    pub date: String,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityScope {
    Global,
    Repo,
    Worktree,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitIdentity {
    pub profile_id: String,
    pub name: String,
    pub email: String,
    pub scope: IdentityScope,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedRef {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub old_sha: String,
    pub new_sha: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictLineKind {
    Ours,
    Theirs,
    ConflictMarker,
    Context,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictLine {
    pub kind: ConflictLineKind,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFile {
    pub path: String,
    pub hunks: Vec<ConflictHunk>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictHunk {
    pub ours_start: u32,
    pub theirs_start: u32,
    pub lines: Vec<ConflictLine>,
}

// ── Git CLI helpers ────────────────────────────────────────────────────

pub(crate) fn git_cmd(ctx: &ExtensionContext) -> Command {
    let mut cmd = Command::new("git");
    if let Some(dir) = &ctx.working_directory {
        cmd.current_dir(dir);
    }
    // The loom server may run detached (e.g. under pm2) with no console of its
    // own; without CREATE_NO_WINDOW every git invocation would allocate a new
    // visible console window that flashes open and closes on exit.
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub(crate) fn require_git_scope(ctx: &ExtensionContext, scope: &str) -> Result<(), ExtensionError> {
    super::auth::check_server_policy(ctx, "git", scope)
}

pub(crate) async fn run_git_apply(
    ctx: &ExtensionContext,
    args: &[&str],
    patch: &str,
) -> Result<(), ExtensionError> {
    use tokio::io::AsyncWriteExt;

    let child = {
        let mut cmd = tokio::process::Command::new("git");
        cmd.args(args)
            .current_dir(
                ctx.working_directory
                    .as_deref()
                    .unwrap_or(std::path::Path::new(".")),
            )
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd.spawn()
            .map_err(|e| ExtensionError {
                code: -32603,
                message: "internal_error".into(),
                data: Some(Value::String(format!("failed to spawn git apply: {e}"))),
            })?
    };

    let mut child = child;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(patch.as_bytes()).await.ok();
    }
    let output = child.wait_with_output().await.map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!("git apply failed: {e}"))),
    })?;

    if !output.status.success() {
        return Err(ExtensionError::invalid_params("patch could not be applied"));
    }
    Ok(())
}

pub(crate) async fn run_git(
    ctx: &ExtensionContext,
    args: &[&str],
) -> Result<String, ExtensionError> {
    let output = git_cmd(ctx)
        .args(args)
        .output()
        .await
        .map_err(|e| ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(format!("failed to spawn git: {e}"))),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not a git repository") || stderr.contains("fatal: not a git") {
            return Err(ExtensionError::not_found("not a git repository"));
        }
        if stderr.contains("does not exist") || stderr.contains("unknown revision") {
            return Err(ExtensionError::not_found(&*stderr));
        }
        return Err(ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(stderr.to_string())),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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

pub(crate) fn sanitize_remote_url(url: &str) -> String {
    if let Some(at_pos) = url.find("://") {
        let scheme = &url[..at_pos + 3];
        let rest = &url[at_pos + 3..];
        if let Some(at) = rest.find('@') {
            return format!("{scheme}{}", &rest[at + 1..]);
        }
    }
    if url.starts_with("git@") {
        return url.to_string();
    }
    if let Some(rest) = url.strip_prefix("ssh://") {
        if let Some(at) = rest.find('@') {
            return format!("ssh://{}", &rest[at + 1..]);
        }
    }
    url.to_string()
}

pub(crate) fn classify_remote_url(url: &str) -> RemoteUrlType {
    if url.starts_with("https://") || url.starts_with("http://") {
        RemoteUrlType::Https
    } else if url.starts_with("git@") || url.starts_with("ssh://") {
        RemoteUrlType::Ssh
    } else {
        RemoteUrlType::File
    }
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

pub(crate) fn parse_porcelain_status_v2(output: &str) -> GitStatus {
    let mut branch = String::new();
    let mut upstream: Option<String> = None;
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut files = Vec::new();
    let mut in_progress: Option<GitInProgress> = None;
    let mut conflict_files = Vec::new();

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            branch = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            upstream = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            for p in parts {
                if let Some(n) = p.strip_prefix('+') {
                    ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = p.strip_prefix('-') {
                    behind = n.parse().unwrap_or(0);
                }
            }
        } else if line.starts_with("u ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let path = parts.last().unwrap_or(&"").to_string();
                conflict_files.push(path.clone());
                files.push(GitStatusFile {
                    path,
                    index_status: GitFileStatus::Unmerged,
                    working_status: GitFileStatus::Unmerged,
                });
            }
        } else if line.starts_with("1 ") {
            let tokens: Vec<&str> = line.splitn(9, ' ').collect();
            if tokens.len() < 9 {
                continue;
            }
            let xy = tokens.get(1).unwrap_or(&"..");
            let index_status = parse_xy_status(xy.chars().next().unwrap_or('.'));
            let working_status = parse_xy_status(xy.chars().nth(1).unwrap_or('.'));
            let path_part = tokens.get(8).unwrap_or(&"");

            if matches!(index_status, GitFileStatus::Unmerged)
                || matches!(working_status, GitFileStatus::Unmerged)
            {
                conflict_files.push(path_part.to_string());
            }

            files.push(GitStatusFile {
                path: path_part.to_string(),
                index_status,
                working_status,
            });
        } else if line.starts_with("2 ") {
            let tokens: Vec<&str> = line.splitn(11, ' ').collect();
            if tokens.len() < 11 {
                continue;
            }
            let xy = tokens.get(1).unwrap_or(&"..");
            let index_status = parse_xy_status(xy.chars().next().unwrap_or('.'));
            let working_status = parse_xy_status(xy.chars().nth(1).unwrap_or('.'));
            let path_field = tokens.get(10).unwrap_or(&"");
            let path_part = path_field.split('\t').next().unwrap_or(path_field);

            if matches!(index_status, GitFileStatus::Unmerged)
                || matches!(working_status, GitFileStatus::Unmerged)
            {
                conflict_files.push(path_part.to_string());
            }

            files.push(GitStatusFile {
                path: path_part.to_string(),
                index_status,
                working_status,
            });
        } else if let Some(path) = line.strip_prefix("? ") {
            let path = path.trim();
            files.push(GitStatusFile {
                path: path.to_string(),
                index_status: GitFileStatus::Untracked,
                working_status: GitFileStatus::Untracked,
            });
        }
    }

    if !conflict_files.is_empty() {
        in_progress = Some(GitInProgress {
            operation: GitOperation::Merge,
            conflict_files,
        });
    }

    GitStatus {
        branch,
        upstream,
        ahead,
        behind,
        files,
        in_progress,
    }
}

fn parse_xy_status(c: char) -> GitFileStatus {
    match c {
        '.' => GitFileStatus::Unmodified,
        'M' => GitFileStatus::Modified,
        'A' => GitFileStatus::Added,
        'D' => GitFileStatus::Deleted,
        'R' => GitFileStatus::Renamed,
        'C' => GitFileStatus::Copied,
        'U' => GitFileStatus::Unmerged,
        '?' => GitFileStatus::Untracked,
        '!' => GitFileStatus::Ignored,
        _ => GitFileStatus::Unmodified,
    }
}

pub(crate) fn parse_diff_output(diff_text: &str, stat_text: &str) -> GitDiffSummary {
    let mut hunks = Vec::new();
    let mut current_hunk_lines: Vec<GitDiffLine> = Vec::new();
    let mut old_path = String::new();
    let mut new_path = String::new();
    let mut old_start = 0u32;
    let mut old_lines = 0u32;
    let mut new_start = 0u32;
    let mut new_lines = 0u32;
    let mut header = String::new();
    let mut in_hunk = false;
    let mut old_line_counter = 0u32;
    let mut new_line_counter = 0u32;

    for line in diff_text.lines() {
        if let Some(rest) = line.strip_prefix("--- ") {
            old_path = rest.trim_start_matches("a/").to_string();
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            new_path = rest.trim_start_matches("b/").to_string();
        } else if line.starts_with("@@") {
            if in_hunk && !current_hunk_lines.is_empty() {
                hunks.push(GitDiffHunk {
                    old_path: old_path.clone(),
                    new_path: new_path.clone(),
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    header: header.clone(),
                    lines: std::mem::take(&mut current_hunk_lines),
                });
            }
            header = line.to_string();
            if let Some((o, n)) = parse_hunk_header(line) {
                old_start = o.0;
                old_lines = o.1;
                new_start = n.0;
                new_lines = n.1;
                old_line_counter = o.0;
                new_line_counter = n.0;
            }
            in_hunk = true;
        } else if in_hunk {
            if let Some(rest) = line.strip_prefix('+') {
                current_hunk_lines.push(GitDiffLine {
                    kind: GitDiffLineKind::Addition,
                    content: rest.to_string(),
                    old_line: None,
                    new_line: Some(new_line_counter),
                });
                new_line_counter += 1;
            } else if let Some(rest) = line.strip_prefix('-') {
                current_hunk_lines.push(GitDiffLine {
                    kind: GitDiffLineKind::Deletion,
                    content: rest.to_string(),
                    old_line: Some(old_line_counter),
                    new_line: None,
                });
                old_line_counter += 1;
            } else if line.starts_with("\\ No newline") {
                current_hunk_lines.push(GitDiffLine {
                    kind: GitDiffLineKind::NoNewline,
                    content: line.to_string(),
                    old_line: None,
                    new_line: None,
                });
            } else if let Some(rest) = line.strip_prefix(' ') {
                current_hunk_lines.push(GitDiffLine {
                    kind: GitDiffLineKind::Context,
                    content: rest.to_string(),
                    old_line: Some(old_line_counter),
                    new_line: Some(new_line_counter),
                });
                old_line_counter += 1;
                new_line_counter += 1;
            }
        }
    }
    if in_hunk && !current_hunk_lines.is_empty() {
        hunks.push(GitDiffHunk {
            old_path,
            new_path,
            old_start,
            old_lines,
            new_start,
            new_lines,
            header,
            lines: current_hunk_lines,
        });
    }

    let stat = parse_diff_stat(stat_text);
    GitDiffSummary { hunks, stat }
}

fn parse_hunk_header(line: &str) -> Option<((u32, u32), (u32, u32))> {
    let start = line.find("@@ ")?;
    let end = line[3..].find(" @@")?;
    let core = &line[start + 3..start + 3 + end];
    let parts: Vec<&str> = core.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let old_part = parts[0].strip_prefix('-')?;
    let new_part = parts[1].strip_prefix('+')?;
    let old_nums: Vec<u32> = old_part.split(',').filter_map(|s| s.parse().ok()).collect();
    let new_nums: Vec<u32> = new_part.split(',').filter_map(|s| s.parse().ok()).collect();
    let old_start = *old_nums.first()?;
    let old_lines = old_nums.get(1).copied().unwrap_or(1);
    let new_start = *new_nums.first()?;
    let new_lines = new_nums.get(1).copied().unwrap_or(1);
    Some(((old_start, old_lines), (new_start, new_lines)))
}

fn parse_diff_stat(stat_text: &str) -> GitDiffStat {
    let mut files_changed = 0u32;
    let mut insertions = 0u32;
    let mut deletions = 0u32;
    let mut last_data_line = "";

    for line in stat_text.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("diff ") {
            continue;
        }
        if trimmed.contains("file")
            || (trimmed.contains("insertion") || trimmed.contains("deletion"))
        {
            last_data_line = trimmed;
            break;
        }
        files_changed += 1;
    }

    if last_data_line.is_empty() {
        return GitDiffStat {
            files_changed,
            insertions,
            deletions,
        };
    }

    let mut last_num = 0u32;
    for token in last_data_line.split_whitespace() {
        if let Ok(n) = token.parse::<u32>() {
            last_num = n;
        } else if token.contains("file") {
            files_changed = last_num;
        } else if token.contains("insertion") {
            insertions = last_num;
        } else if token.contains("deletion") {
            deletions = last_num;
        }
    }

    if files_changed == 0 && !hunks_text_is_empty(stat_text) {
        files_changed = 1;
    }

    GitDiffStat {
        files_changed,
        insertions,
        deletions,
    }
}

fn hunks_text_is_empty(s: &str) -> bool {
    s.lines().all(|l| l.trim().is_empty())
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
