//! Backend selection (`ANUREO_GIT_BACKEND=git2|cli`) + method-level delegation.
//!
//! Transition strategy (docs/design/git2-migration.md §4): the git2 backend
//! delegates unimplemented methods to CliBackend, so the git2 default is
//! always functionally complete. The delegation surface shrinks to zero by B4.
//!
//! Remote ops (§6.2): git2 credential callbacks first (GITHUB_TOKEN PAT /
//! ~/.ssh/id_*); on auth-classified failure the facade falls back to
//! CliBackend (`git push`/`git fetch` with GCM/ssh-agent) and tags
//! `push_via`/`fetch_via: "cli-fallback"`.

use std::path::Path;
use std::sync::Arc;

use crate::backend::{GitBackend, LogQuery};
use crate::cli::CliBackend;
use crate::error::{GitError, GitErrorKind, GitResult};
use crate::git2_backend::Git2Backend;
use crate::types::{
    CommitFileEntry, CommitRequest, CommitResult, FetchResult, GitBranch, GitCommitInfo,
    GitDiffSummary, GitInProgress, GitRemote, GitStashEntry, GitStatus, MergeOptions, MergeResult,
    PullResult, PushResult, RebaseResult, StashCountResult, StashPushOptions, WorktreeInfo,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Git2,
    Cli,
}

impl BackendKind {
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("cli") => BackendKind::Cli,
            _ => BackendKind::Git2,
        }
    }
}

pub fn backend_kind() -> BackendKind {
    static KIND: std::sync::OnceLock<BackendKind> = std::sync::OnceLock::new();
    *KIND.get_or_init(|| BackendKind::parse(std::env::var("ANUREO_GIT_BACKEND").ok().as_deref()))
}

/// Primary backend per the env flag.
pub fn primary_backend() -> Arc<dyn GitBackend> {
    match backend_kind() {
        BackendKind::Cli => Arc::new(CliBackend),
        BackendKind::Git2 => Arc::new(Git2Backend),
    }
}

pub fn cli_backend() -> Arc<CliBackend> {
    static CLI: std::sync::OnceLock<Arc<CliBackend>> = std::sync::OnceLock::new();
    CLI.get_or_init(|| Arc::new(CliBackend)).clone()
}

/// Primary -> CliBackend on Unsupported (method-level delegation).
/// The Unsupported arm is unreachable once Git2Backend implements the full
/// trait surface; kept for the transition contract (§4).
#[cfg_attr(coverage, coverage(off))]
async fn delegated<T>(
    op: &str,
    f: impl std::future::Future<Output = GitResult<T>>,
    fallback: impl std::future::Future<Output = GitResult<T>>,
) -> GitResult<T> {
    match f.await {
        Err(e) if e.is_unsupported() => {
            tracing::debug!(
                operation = op,
                "git2 backend unsupported, delegating to cli"
            );
            fallback.await
        }
        result => result,
    }
}

/// index.lock backoff retry (100/200/400ms) for write ops.
/// Closures must be re-invocable; capture owned data.
async fn lock_retry<T, F>(mut f: F) -> GitResult<T>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = GitResult<T>> + Send>> + Send,
{
    let delays = [100u64, 200, 400];
    let mut attempt = 0;
    loop {
        match f().await {
            Err(e) if e.is_locked() && attempt < delays.len() => {
                tracing::warn!(
                    attempt = attempt + 1,
                    delay_ms = delays[attempt],
                    "index.lock contention, retrying"
                );
                tokio::time::sleep(std::time::Duration::from_millis(delays[attempt])).await;
                attempt += 1;
            }
            r => return r,
        }
    }
}

fn is_auth(e: &GitError) -> bool {
    matches!(e.kind(), GitErrorKind::Auth)
}

pub async fn run_raw(workdir: Option<&Path>, args: &[&str]) -> GitResult<String> {
    let primary = primary_backend();
    let cli = cli_backend();
    match primary.raw(workdir, args).await {
        Err(e) if e.is_unsupported() => cli.raw(workdir, args).await,
        result => result,
    }
}

pub async fn run_apply_raw(workdir: Option<&Path>, args: &[&str], patch: &str) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    match primary.apply(workdir, args, patch).await {
        Err(e) if e.is_unsupported() => cli.apply(workdir, args, patch).await,
        result => result,
    }
}

pub async fn status(repo: &Path) -> GitResult<GitStatus> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("status", primary.status(repo), async move {
        cli.status(repo).await
    })
    .await
}

pub async fn log(repo: &Path, query: &LogQuery) -> GitResult<Vec<GitCommitInfo>> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("log", primary.log(repo, query), async move {
        cli.log(repo, query).await
    })
    .await
}

pub async fn branches(repo: &Path, remote: bool) -> GitResult<Vec<GitBranch>> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("branches", primary.branches(repo, remote), async move {
        cli.branches(repo, remote).await
    })
    .await
}

pub async fn diff(
    repo: &Path,
    staged: bool,
    path: Option<&str>,
    unified: u32,
) -> GitResult<GitDiffSummary> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated(
        "diff",
        primary.diff(repo, staged, path, unified),
        async move { cli.diff(repo, staged, path, unified).await },
    )
    .await
}

pub async fn remotes(repo: &Path) -> GitResult<Vec<GitRemote>> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("remotes", primary.remotes(repo), async move {
        cli.remotes(repo).await
    })
    .await
}

pub async fn in_progress(repo: &Path) -> GitResult<Option<GitInProgress>> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("in_progress", primary.in_progress(repo), async move {
        cli.in_progress(repo).await
    })
    .await
}

async fn stage_file_direct(repo: &Path, path: &str) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    let p1 = path.to_string();
    let p2 = p1.clone();
    delegated("stage_file", primary.stage_file(repo, &p1), async move {
        cli.stage_file(repo, &p2).await
    })
    .await
}
pub async fn stage_file(repo: &Path, path: &str) -> GitResult<()> {
    let repo = repo.to_path_buf();
    let path = path.to_string();
    lock_retry(move || {
        let repo = repo.clone();
        let path = path.clone();
        Box::pin(async move { stage_file_direct(&repo, &path).await })
    })
    .await
}

async fn unstage_file_direct(repo: &Path, path: &str) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    let p1 = path.to_string();
    let p2 = p1.clone();
    delegated(
        "unstage_file",
        primary.unstage_file(repo, &p1),
        async move { cli.unstage_file(repo, &p2).await },
    )
    .await
}
pub async fn unstage_file(repo: &Path, path: &str) -> GitResult<()> {
    let repo = repo.to_path_buf();
    let path = path.to_string();
    lock_retry(move || {
        let repo = repo.clone();
        let path = path.clone();
        Box::pin(async move { unstage_file_direct(&repo, &path).await })
    })
    .await
}

async fn commit_direct(repo: &Path, req: CommitRequest) -> GitResult<CommitResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("commit", primary.commit(repo, req.clone()), async move {
        cli.commit(repo, req).await
    })
    .await
}
pub async fn commit(repo: &Path, req: CommitRequest) -> GitResult<CommitResult> {
    let repo = repo.to_path_buf();
    let req = req.clone();
    lock_retry(move || {
        let repo = repo.clone();
        let req = req.clone();
        Box::pin(async move { commit_direct(&repo, req).await })
    })
    .await
}

pub async fn stash_list(repo: &Path) -> GitResult<Vec<GitStashEntry>> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("stash_list", primary.stash_list(repo), async move {
        cli.stash_list(repo).await
    })
    .await
}

pub async fn stash_count(repo: &Path) -> GitResult<StashCountResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("stash_count", primary.stash_count(repo), async move {
        cli.stash_count(repo).await
    })
    .await
}

pub async fn commit_files(repo: &Path, commit: &str) -> GitResult<Vec<CommitFileEntry>> {
    let primary = primary_backend();
    let cli = cli_backend();
    let c1 = commit.to_string();
    let c2 = c1.clone();
    delegated(
        "commit_files",
        primary.commit_files(repo, &c1),
        async move { cli.commit_files(repo, &c2).await },
    )
    .await
}

pub async fn commit_file_diff(
    repo: &Path,
    commit: &str,
    path: &str,
    unified: u32,
) -> GitResult<GitDiffSummary> {
    let primary = primary_backend();
    let cli = cli_backend();
    let c1 = commit.to_string();
    let p1 = path.to_string();
    let c2 = c1.clone();
    let p2 = p1.clone();
    delegated(
        "commit_file_diff",
        primary.commit_file_diff(repo, &c1, &p1, unified),
        async move { cli.commit_file_diff(repo, &c2, &p2, unified).await },
    )
    .await
}

pub async fn remote_url(repo: &Path, remote: &str) -> GitResult<String> {
    let primary = primary_backend();
    let cli = cli_backend();
    let r1 = remote.to_string();
    let r2 = r1.clone();
    delegated("remote_url", primary.remote_url(repo, &r1), async move {
        cli.remote_url(repo, &r2).await
    })
    .await
}

pub async fn config_get(repo: &Path, key: &str) -> GitResult<Option<String>> {
    let primary = primary_backend();
    let cli = cli_backend();
    let k1 = key.to_string();
    let k2 = k1.clone();
    delegated("config_get", primary.config_get(repo, &k1), async move {
        cli.config_get(repo, &k2).await
    })
    .await
}

async fn config_set_direct(repo: &Path, key: &str, value: &str, global: bool) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    let k1 = key.to_string();
    let v1 = value.to_string();
    let k2 = k1.clone();
    let v2 = v1.clone();
    delegated(
        "config_set",
        primary.config_set(repo, &k1, &v1, global),
        async move { cli.config_set(repo, &k2, &v2, global).await },
    )
    .await
}
pub async fn config_set(repo: &Path, key: &str, value: &str, global: bool) -> GitResult<()> {
    let repo = repo.to_path_buf();
    let key = key.to_string();
    let value = value.to_string();
    lock_retry(move || {
        let repo = repo.clone();
        let key = key.clone();
        let value = value.clone();
        Box::pin(async move { config_set_direct(&repo, &key, &value, global).await })
    })
    .await
}

async fn checkout_branch_direct(repo: &Path, branch: &str) -> GitResult<String> {
    let primary = primary_backend();
    let cli = cli_backend();
    let b1 = branch.to_string();
    let b2 = b1.clone();
    delegated(
        "checkout_branch",
        primary.checkout_branch(repo, &b1),
        async move { cli.checkout_branch(repo, &b2).await },
    )
    .await
}
pub async fn checkout_branch(repo: &Path, branch: &str) -> GitResult<String> {
    let repo = repo.to_path_buf();
    let branch = branch.to_string();
    lock_retry(move || {
        let repo = repo.clone();
        let branch = branch.clone();
        Box::pin(async move { checkout_branch_direct(&repo, &branch).await })
    })
    .await
}

async fn reset_to_commit_direct(repo: &Path, commit: &str, mode: &str) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    let c1 = commit.to_string();
    let m1 = mode.to_string();
    let c2 = c1.clone();
    let m2 = m1.clone();
    delegated(
        "reset_to_commit",
        primary.reset_to_commit(repo, &c1, &m1),
        async move { cli.reset_to_commit(repo, &c2, &m2).await },
    )
    .await
}
pub async fn reset_to_commit(repo: &Path, commit: &str, mode: &str) -> GitResult<()> {
    let repo = repo.to_path_buf();
    let commit = commit.to_string();
    let mode = mode.to_string();
    lock_retry(move || {
        let repo = repo.clone();
        let commit = commit.clone();
        let mode = mode.clone();
        Box::pin(async move { reset_to_commit_direct(&repo, &commit, &mode).await })
    })
    .await
}

async fn stage_hunk_direct(repo: &Path, patch: &str) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    let p1 = patch.to_string();
    let p2 = p1.clone();
    delegated("stage_hunk", primary.stage_hunk(repo, &p1), async move {
        cli.stage_hunk(repo, &p2).await
    })
    .await
}
pub async fn stage_hunk(repo: &Path, patch: &str) -> GitResult<()> {
    let repo = repo.to_path_buf();
    let patch = patch.to_string();
    lock_retry(move || {
        let repo = repo.clone();
        let patch = patch.clone();
        Box::pin(async move { stage_hunk_direct(&repo, &patch).await })
    })
    .await
}

async fn unstage_hunk_direct(repo: &Path, patch: &str) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    let p1 = patch.to_string();
    let p2 = p1.clone();
    delegated(
        "unstage_hunk",
        primary.unstage_hunk(repo, &p1),
        async move { cli.unstage_hunk(repo, &p2).await },
    )
    .await
}
pub async fn unstage_hunk(repo: &Path, patch: &str) -> GitResult<()> {
    let repo = repo.to_path_buf();
    let patch = patch.to_string();
    lock_retry(move || {
        let repo = repo.clone();
        let patch = patch.clone();
        Box::pin(async move { unstage_hunk_direct(&repo, &patch).await })
    })
    .await
}

async fn revert_hunk_direct(repo: &Path, patch: &str) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    let p1 = patch.to_string();
    let p2 = p1.clone();
    delegated("revert_hunk", primary.revert_hunk(repo, &p1), async move {
        cli.revert_hunk(repo, &p2).await
    })
    .await
}
pub async fn revert_hunk(repo: &Path, patch: &str) -> GitResult<()> {
    let repo = repo.to_path_buf();
    let patch = patch.to_string();
    lock_retry(move || {
        let repo = repo.clone();
        let patch = patch.clone();
        Box::pin(async move { revert_hunk_direct(&repo, &patch).await })
    })
    .await
}

async fn stash_push_direct(repo: &Path, opts: StashPushOptions) -> GitResult<bool> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated(
        "stash_push",
        primary.stash_push(repo, opts.clone()),
        async move { cli.stash_push(repo, opts).await },
    )
    .await
}
pub async fn stash_push(repo: &Path, opts: StashPushOptions) -> GitResult<bool> {
    let repo = repo.to_path_buf();
    let opts = opts.clone();
    lock_retry(move || {
        let repo = repo.clone();
        let opts = opts.clone();
        Box::pin(async move { stash_push_direct(&repo, opts).await })
    })
    .await
}

async fn stash_pop_direct(repo: &Path, index: usize) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("stash_pop", primary.stash_pop(repo, index), async move {
        cli.stash_pop(repo, index).await
    })
    .await
}
pub async fn stash_pop(repo: &Path, index: usize) -> GitResult<()> {
    let repo = repo.to_path_buf();
    lock_retry(move || {
        let repo = repo.clone();
        Box::pin(async move { stash_pop_direct(&repo, index).await })
    })
    .await
}

async fn stash_apply_direct(repo: &Path, index: usize) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated(
        "stash_apply",
        primary.stash_apply(repo, index),
        async move { cli.stash_apply(repo, index).await },
    )
    .await
}
pub async fn stash_apply(repo: &Path, index: usize) -> GitResult<()> {
    let repo = repo.to_path_buf();
    lock_retry(move || {
        let repo = repo.clone();
        Box::pin(async move { stash_apply_direct(&repo, index).await })
    })
    .await
}

async fn stash_drop_direct(repo: &Path, index: usize) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("stash_drop", primary.stash_drop(repo, index), async move {
        cli.stash_drop(repo, index).await
    })
    .await
}
pub async fn stash_drop(repo: &Path, index: usize) -> GitResult<()> {
    let repo = repo.to_path_buf();
    lock_retry(move || {
        let repo = repo.clone();
        Box::pin(async move { stash_drop_direct(&repo, index).await })
    })
    .await
}

pub async fn stash_show(repo: &Path, index: usize) -> GitResult<StashCountResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("stash_show", primary.stash_show(repo, index), async move {
        cli.stash_show(repo, index).await
    })
    .await
}

async fn merge_direct(repo: &Path, branch: &str, opts: MergeOptions) -> GitResult<MergeResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    let b1 = branch.to_string();
    let b2 = b1.clone();
    delegated(
        "merge",
        primary.merge(repo, &b1, opts.clone()),
        async move { cli.merge(repo, &b2, opts).await },
    )
    .await
}
pub async fn merge(repo: &Path, branch: &str, opts: MergeOptions) -> GitResult<MergeResult> {
    let repo = repo.to_path_buf();
    let branch = branch.to_string();
    let opts = opts.clone();
    lock_retry(move || {
        let repo = repo.clone();
        let branch = branch.clone();
        let opts = opts.clone();
        Box::pin(async move { merge_direct(&repo, &branch, opts).await })
    })
    .await
}

async fn merge_continue_direct(repo: &Path, message: Option<&str>) -> GitResult<MergeResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    let m1 = message.map(|m| m.to_string());
    let m2 = m1.clone();
    delegated(
        "merge_continue",
        primary.merge_continue(repo, m1.as_deref()),
        async move { cli.merge_continue(repo, m2.as_deref()).await },
    )
    .await
}
pub async fn merge_continue(repo: &Path, message: Option<&str>) -> GitResult<MergeResult> {
    let repo = repo.to_path_buf();
    let message = message.map(|m| m.to_string());
    lock_retry(move || {
        let repo = repo.clone();
        let message = message.clone();
        Box::pin(async move { merge_continue_direct(&repo, message.as_deref()).await })
    })
    .await
}

async fn merge_abort_direct(repo: &Path) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("merge_abort", primary.merge_abort(repo), async move {
        cli.merge_abort(repo).await
    })
    .await
}
pub async fn merge_abort(repo: &Path) -> GitResult<()> {
    let repo = repo.to_path_buf();
    lock_retry(move || {
        let repo = repo.clone();
        Box::pin(async move { merge_abort_direct(&repo).await })
    })
    .await
}

async fn rebase_direct(repo: &Path, onto: &str) -> GitResult<RebaseResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    let o1 = onto.to_string();
    let o2 = o1.clone();
    delegated("rebase", primary.rebase(repo, &o1), async move {
        cli.rebase(repo, &o2).await
    })
    .await
}
pub async fn rebase(repo: &Path, onto: &str) -> GitResult<RebaseResult> {
    let repo = repo.to_path_buf();
    let onto = onto.to_string();
    lock_retry(move || {
        let repo = repo.clone();
        let onto = onto.clone();
        Box::pin(async move { rebase_direct(&repo, &onto).await })
    })
    .await
}

async fn rebase_continue_direct(repo: &Path, message: Option<&str>) -> GitResult<RebaseResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    let m1 = message.map(|m| m.to_string());
    let m2 = m1.clone();
    delegated(
        "rebase_continue",
        primary.rebase_continue(repo, m1.as_deref()),
        async move { cli.rebase_continue(repo, m2.as_deref()).await },
    )
    .await
}
pub async fn rebase_continue(repo: &Path, message: Option<&str>) -> GitResult<RebaseResult> {
    let repo = repo.to_path_buf();
    let message = message.map(|m| m.to_string());
    lock_retry(move || {
        let repo = repo.clone();
        let message = message.clone();
        Box::pin(async move { rebase_continue_direct(&repo, message.as_deref()).await })
    })
    .await
}

async fn rebase_skip_direct(repo: &Path) -> GitResult<RebaseResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("rebase_skip", primary.rebase_skip(repo), async move {
        cli.rebase_skip(repo).await
    })
    .await
}
pub async fn rebase_skip(repo: &Path) -> GitResult<RebaseResult> {
    let repo = repo.to_path_buf();
    lock_retry(move || {
        let repo = repo.clone();
        Box::pin(async move { rebase_skip_direct(&repo).await })
    })
    .await
}

async fn rebase_abort_direct(repo: &Path) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("rebase_abort", primary.rebase_abort(repo), async move {
        cli.rebase_abort(repo).await
    })
    .await
}
pub async fn rebase_abort(repo: &Path) -> GitResult<()> {
    let repo = repo.to_path_buf();
    lock_retry(move || {
        let repo = repo.clone();
        Box::pin(async move { rebase_abort_direct(&repo).await })
    })
    .await
}

async fn cherry_pick_direct(repo: &Path, commit: &str, no_commit: bool) -> GitResult<MergeResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    let c1 = commit.to_string();
    let c2 = c1.clone();
    delegated(
        "cherry_pick",
        primary.cherry_pick(repo, &c1, no_commit),
        async move { cli.cherry_pick(repo, &c2, no_commit).await },
    )
    .await
}
pub async fn cherry_pick(repo: &Path, commit: &str, no_commit: bool) -> GitResult<MergeResult> {
    let repo = repo.to_path_buf();
    let commit = commit.to_string();
    lock_retry(move || {
        let repo = repo.clone();
        let commit = commit.clone();
        Box::pin(async move { cherry_pick_direct(&repo, &commit, no_commit).await })
    })
    .await
}

async fn revert_commit_direct(repo: &Path, commit: &str) -> GitResult<MergeResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    let c1 = commit.to_string();
    let c2 = c1.clone();
    delegated(
        "revert_commit",
        primary.revert_commit(repo, &c1),
        async move { cli.revert_commit(repo, &c2).await },
    )
    .await
}
pub async fn revert_commit(repo: &Path, commit: &str) -> GitResult<MergeResult> {
    let repo = repo.to_path_buf();
    let commit = commit.to_string();
    lock_retry(move || {
        let repo = repo.clone();
        let commit = commit.clone();
        Box::pin(async move { revert_commit_direct(&repo, &commit).await })
    })
    .await
}

pub async fn fetch(
    repo: &Path,
    remote: &str,
    branch: Option<&str>,
    prune: bool,
) -> GitResult<FetchResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    let r = remote.to_string();
    let b = branch.map(|s| s.to_string());
    match primary.fetch(repo, &r, b.as_deref(), prune).await {
        Err(e) if is_auth(&e) => {
            tracing::warn!(remote = %r, "git2 fetch auth failed, falling back to git CLI");
            let mut res = cli
                .fetch(repo, &r, b.as_deref(), prune)
                .await
                .map_err(|e2| {
                    if matches!(e2.kind(), GitErrorKind::GitMissing) {
                        GitError::new(
                            GitErrorKind::Auth,
                            format!(
                                "fetch needs credentials (set GITHUB_TOKEN or install git); \
                                 git2: {}; cli: {}",
                                e.message(),
                                e2.message()
                            ),
                        )
                    } else {
                        e2
                    }
                })?;
            res.fetch_via = Some("cli-fallback");
            Ok(res)
        }
        Err(e) if e.is_unsupported() => cli.fetch(repo, &r, b.as_deref(), prune).await,
        result => result,
    }
}

pub async fn push(
    repo: &Path,
    remote: &str,
    branch: &str,
    force: bool,
    set_upstream: bool,
) -> GitResult<PushResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    let r = remote.to_string();
    let b = branch.to_string();
    match primary.push(repo, &r, &b, force, set_upstream).await {
        Err(e) if is_auth(&e) => {
            tracing::warn!(remote = %r, "git2 push auth failed, falling back to git CLI");
            let mut res = cli
                .push(repo, &r, &b, force, set_upstream)
                .await
                .map_err(|e2| {
                    if matches!(e2.kind(), GitErrorKind::GitMissing) {
                        GitError::new(
                            GitErrorKind::Auth,
                            format!(
                                "push needs credentials (set GITHUB_TOKEN or install git); \
                                 git2: {}; cli: {}",
                                e.message(),
                                e2.message()
                            ),
                        )
                    } else {
                        e2
                    }
                })?;
            res.push_via = Some("cli-fallback");
            Ok(res)
        }
        Err(e) if e.is_unsupported() => cli.push(repo, &r, &b, force, set_upstream).await,
        result => result,
    }
}

async fn pull_direct(repo: &Path, remote: &str, branch: Option<&str>) -> GitResult<PullResult> {
    let primary = primary_backend();
    let cli = cli_backend();
    let r1 = remote.to_string();
    let b1 = branch.map(|s| s.to_string());
    let r2 = r1.clone();
    let b2 = b1.clone();
    delegated("pull", primary.pull(repo, &r1, b1.as_deref()), async move {
        cli.pull(repo, &r2, b2.as_deref()).await
    })
    .await
}
pub async fn pull(repo: &Path, remote: &str, branch: Option<&str>) -> GitResult<PullResult> {
    let repo = repo.to_path_buf();
    let remote = remote.to_string();
    let branch = branch.map(|s| s.to_string());
    lock_retry(move || {
        let repo = repo.clone();
        let remote = remote.clone();
        let branch = branch.clone();
        Box::pin(async move { pull_direct(&repo, &remote, branch.as_deref()).await })
    })
    .await
}

pub async fn worktree_list(repo: &Path) -> GitResult<Vec<WorktreeInfo>> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("worktree_list", primary.worktree_list(repo), async move {
        cli.worktree_list(repo).await
    })
    .await
}

async fn worktree_add_direct(
    repo: &Path,
    branch: &str,
    path: &Path,
    create_branch: bool,
) -> GitResult<()> {
    let primary = primary_backend();
    let cli = cli_backend();
    let b1 = branch.to_string();
    let p1 = path.to_path_buf();
    let b2 = b1.clone();
    let p2 = p1.clone();
    delegated(
        "worktree_add",
        primary.worktree_add(repo, &b1, &p1, create_branch),
        async move { cli.worktree_add(repo, &b2, &p2, create_branch).await },
    )
    .await
}
pub async fn worktree_add(
    repo: &Path,
    branch: &str,
    path: &Path,
    create_branch: bool,
) -> GitResult<()> {
    let repo = repo.to_path_buf();
    let branch = branch.to_string();
    let path = path.to_path_buf();
    lock_retry(move || {
        let repo = repo.clone();
        let branch = branch.clone();
        let path = path.clone();
        Box::pin(async move { worktree_add_direct(&repo, &branch, &path, create_branch).await })
    })
    .await
}

pub async fn is_linked_worktree(repo: &Path) -> GitResult<bool> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated(
        "is_linked_worktree",
        primary.is_linked_worktree(repo),
        async move { cli.is_linked_worktree(repo).await },
    )
    .await
}

pub async fn is_dirty(repo: &Path) -> GitResult<bool> {
    let primary = primary_backend();
    let cli = cli_backend();
    delegated("is_dirty", primary.is_dirty(repo), async move {
        cli.is_dirty(repo).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_kind_parsing() {
        assert_eq!(BackendKind::parse(None), BackendKind::Git2);
        assert_eq!(BackendKind::parse(Some("")), BackendKind::Git2);
        assert_eq!(BackendKind::parse(Some("git2")), BackendKind::Git2);
        assert_eq!(BackendKind::parse(Some("GIT2")), BackendKind::Git2);
        assert_eq!(BackendKind::parse(Some("cli")), BackendKind::Cli);
        assert_eq!(BackendKind::parse(Some(" CLI ")), BackendKind::Cli);
        assert_eq!(BackendKind::parse(Some("bogus")), BackendKind::Git2);
    }

    struct UnsupportedBackend;

    #[async_trait::async_trait]
    impl GitBackend for UnsupportedBackend {
        fn name(&self) -> &'static str {
            "unsupported-test"
        }
    }

    #[tokio::test]
    async fn unsupported_delegates_to_cli() {
        let backend: Arc<dyn GitBackend> = Arc::new(UnsupportedBackend);
        let cli = cli_backend();
        let result = delegated(
            "raw",
            backend.raw(None, &["rev-parse", "--git-dir"]),
            async move { cli.raw(None, &["rev-parse", "--git-dir"]).await },
        )
        .await;
        if let Err(e) = result {
            assert!(!e.is_unsupported(), "fallback must execute");
        }
    }
}
