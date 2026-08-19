//! Typed structs matching the extension JSON contract byte-for-byte.
//! Copied from apps/acp/src/extensions/git/mod.rs — serde attrs must not drift.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitOperation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusFile {
    pub path: String,
    pub index_status: GitFileStatus,
    pub working_status: GitFileStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInProgress {
    pub operation: GitOperation,
    pub conflict_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<GitStatusFile>,
    pub in_progress: Option<GitInProgress>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitDiffLineKind {
    Context,
    Addition,
    Deletion,
    NoNewline,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffLine {
    pub kind: GitDiffLineKind,
    pub content: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffStat {
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffSummary {
    pub hunks: Vec<GitDiffHunk>,
    pub stat: GitDiffStat,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteUrlType {
    Https,
    Ssh,
    File,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRemote {
    pub name: String,
    pub url: String,
    pub url_type: RemoteUrlType,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStashEntry {
    pub index: u32,
    pub message: String,
    pub date: String,
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityScope {
    Global,
    Repo,
    Worktree,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitIdentity {
    pub profile_id: String,
    pub name: String,
    pub email: String,
    pub scope: IdentityScope,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedRef {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub old_sha: String,
    pub new_sha: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictLineKind {
    Ours,
    Theirs,
    ConflictMarker,
    Context,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictLine {
    pub kind: ConflictLineKind,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFile {
    pub path: String,
    pub hunks: Vec<ConflictHunk>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictHunk {
    pub ours_start: u32,
    pub theirs_start: u32,
    pub lines: Vec<ConflictLine>,
}

pub fn sanitize_remote_url(url: &str) -> String {
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

pub fn classify_remote_url(url: &str) -> RemoteUrlType {
    if url.starts_with("https://") || url.starts_with("http://") {
        RemoteUrlType::Https
    } else if url.starts_with("git@") || url.starts_with("ssh://") {
        RemoteUrlType::Ssh
    } else {
        RemoteUrlType::File
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommitRequest {
    pub message: String,
    pub amend: bool,
    pub signoff: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitResult {
    pub sha: String,
    pub branch: String,
    pub message: String,
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsigned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<HookOutcome>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashCountFile {
    pub path: String,
    pub insertions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashCountResult {
    pub count: usize,
    pub files: Vec<StashCountFile>,
}

#[derive(Debug, Clone, Default)]
pub struct MergeOptions {
    pub squash: bool,
    pub message: Option<String>,
    pub no_ff: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MergeResult {
    pub fast_forward: bool,
    pub merge_commit: Option<String>,
    pub conflicts: Vec<String>,
    pub conflicted: bool,
    pub squashed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RebaseResult {
    pub conflicts: Vec<String>,
    pub conflicted: bool,
    pub completed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct StashPushOptions {
    pub message: Option<String>,
    pub include_untracked: bool,
    pub keep_index: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PushResult {
    pub remote_sha: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_via: Option<&'static str>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FetchResult {
    pub fetched_refs: Vec<FetchedRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_via: Option<&'static str>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PullResult {
    pub fast_forward: bool,
    pub merge_commit: Option<String>,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitFileEntry {
    pub path: String,
    pub insertions: Option<u32>,
    pub deletions: Option<u32>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HookOutcome {
    pub hooks_present: bool,
    pub hooks_executed: bool,
    pub hooks_skipped_no_sh: bool,
    pub failure: Option<String>,
}
