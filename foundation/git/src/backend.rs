//! `GitBackend` trait. Default bodies return `Unsupported`; the facade treats
//! `Unsupported` from the primary backend as a signal to delegate to CliBackend.

use std::path::Path;

use async_trait::async_trait;

use crate::error::GitResult;
use crate::types::{
    CommitRequest, CommitResult, GitBranch, GitCommitInfo, GitDiffSummary, GitInProgress,
    GitRemote, GitStashEntry, GitStatus, StashCountResult,
};

#[derive(Debug, Clone, Default)]
pub struct LogQuery {
    pub limit: usize,
    pub skip: usize,
    pub branch: Option<String>,
    pub file_path: Option<String>,
}

#[async_trait]
pub trait GitBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Escape hatch: run an arbitrary git subcommand, stdout as string.
    async fn raw(&self, workdir: Option<&Path>, args: &[&str]) -> GitResult<String> {
        let _ = (workdir, args);
        Err(crate::error::GitError::unsupported("raw"))
    }

    /// Feed a patch on stdin (git apply style).
    async fn apply(&self, workdir: Option<&Path>, args: &[&str], patch: &str) -> GitResult<()> {
        let _ = (workdir, args, patch);
        Err(crate::error::GitError::unsupported("apply"))
    }

    async fn status(&self, repo: &Path) -> GitResult<GitStatus> {
        let _ = repo;
        Err(crate::error::GitError::unsupported("status"))
    }

    async fn log(&self, repo: &Path, query: &LogQuery) -> GitResult<Vec<GitCommitInfo>> {
        let _ = (repo, query);
        Err(crate::error::GitError::unsupported("log"))
    }

    async fn branches(&self, repo: &Path, remote: bool) -> GitResult<Vec<GitBranch>> {
        let _ = (repo, remote);
        Err(crate::error::GitError::unsupported("branches"))
    }

    async fn diff(
        &self,
        repo: &Path,
        staged: bool,
        path: Option<&str>,
        unified: u32,
    ) -> GitResult<GitDiffSummary> {
        let _ = (repo, staged, path, unified);
        Err(crate::error::GitError::unsupported("diff"))
    }

    async fn remotes(&self, repo: &Path) -> GitResult<Vec<GitRemote>> {
        let _ = repo;
        Err(crate::error::GitError::unsupported("remotes"))
    }

    async fn in_progress(&self, repo: &Path) -> GitResult<Option<GitInProgress>> {
        let _ = repo;
        Err(crate::error::GitError::unsupported("in_progress"))
    }

    async fn stage_file(&self, repo: &Path, path: &str) -> GitResult<()> {
        let _ = (repo, path);
        Err(crate::error::GitError::unsupported("stage_file"))
    }

    async fn unstage_file(&self, repo: &Path, path: &str) -> GitResult<()> {
        let _ = (repo, path);
        Err(crate::error::GitError::unsupported("unstage_file"))
    }

    async fn commit(&self, repo: &Path, req: CommitRequest) -> GitResult<CommitResult> {
        let _ = (repo, req);
        Err(crate::error::GitError::unsupported("commit"))
    }

    async fn stash_list(&self, repo: &Path) -> GitResult<Vec<GitStashEntry>> {
        let _ = repo;
        Err(crate::error::GitError::unsupported("stash_list"))
    }

    async fn stash_count(&self, repo: &Path) -> GitResult<StashCountResult> {
        let _ = repo;
        Err(crate::error::GitError::unsupported("stash_count"))
    }

    async fn commit_files(
        &self,
        repo: &Path,
        commit: &str,
    ) -> GitResult<Vec<crate::types::CommitFileEntry>> {
        let _ = (repo, commit);
        Err(crate::error::GitError::unsupported("commit_files"))
    }

    async fn commit_file_diff(
        &self,
        repo: &Path,
        commit: &str,
        path: &str,
        unified: u32,
    ) -> GitResult<GitDiffSummary> {
        let _ = (repo, commit, path, unified);
        Err(crate::error::GitError::unsupported("commit_file_diff"))
    }

    async fn remote_url(&self, repo: &Path, remote: &str) -> GitResult<String> {
        let _ = (repo, remote);
        Err(crate::error::GitError::unsupported("remote_url"))
    }

    async fn config_get(&self, repo: &Path, key: &str) -> GitResult<Option<String>> {
        let _ = (repo, key);
        Err(crate::error::GitError::unsupported("config_get"))
    }

    async fn config_set(&self, repo: &Path, key: &str, value: &str, global: bool) -> GitResult<()> {
        let _ = (repo, key, value, global);
        Err(crate::error::GitError::unsupported("config_set"))
    }

    async fn checkout_branch(&self, repo: &Path, branch: &str) -> GitResult<String> {
        let _ = (repo, branch);
        Err(crate::error::GitError::unsupported("checkout_branch"))
    }

    async fn reset_to_commit(&self, repo: &Path, commit: &str, mode: &str) -> GitResult<()> {
        let _ = (repo, commit, mode);
        Err(crate::error::GitError::unsupported("reset_to_commit"))
    }

    async fn stage_hunk(&self, repo: &Path, patch: &str) -> GitResult<()> {
        let _ = (repo, patch);
        Err(crate::error::GitError::unsupported("stage_hunk"))
    }

    async fn unstage_hunk(&self, repo: &Path, patch: &str) -> GitResult<()> {
        let _ = (repo, patch);
        Err(crate::error::GitError::unsupported("unstage_hunk"))
    }

    async fn revert_hunk(&self, repo: &Path, patch: &str) -> GitResult<()> {
        let _ = (repo, patch);
        Err(crate::error::GitError::unsupported("revert_hunk"))
    }

    async fn stash_push(
        &self,
        repo: &Path,
        opts: crate::types::StashPushOptions,
    ) -> GitResult<bool> {
        let _ = (repo, opts);
        Err(crate::error::GitError::unsupported("stash_push"))
    }

    async fn stash_pop(&self, repo: &Path, index: usize) -> GitResult<()> {
        let _ = (repo, index);
        Err(crate::error::GitError::unsupported("stash_pop"))
    }

    async fn stash_apply(&self, repo: &Path, index: usize) -> GitResult<()> {
        let _ = (repo, index);
        Err(crate::error::GitError::unsupported("stash_apply"))
    }

    async fn stash_drop(&self, repo: &Path, index: usize) -> GitResult<()> {
        let _ = (repo, index);
        Err(crate::error::GitError::unsupported("stash_drop"))
    }

    async fn stash_show(
        &self,
        repo: &Path,
        index: usize,
    ) -> GitResult<crate::types::StashCountResult> {
        let _ = (repo, index);
        Err(crate::error::GitError::unsupported("stash_show"))
    }

    async fn merge(
        &self,
        repo: &Path,
        branch: &str,
        opts: crate::types::MergeOptions,
    ) -> GitResult<crate::types::MergeResult> {
        let _ = (repo, branch, opts);
        Err(crate::error::GitError::unsupported("merge"))
    }

    async fn merge_continue(
        &self,
        repo: &Path,
        message: Option<&str>,
    ) -> GitResult<crate::types::MergeResult> {
        let _ = (repo, message);
        Err(crate::error::GitError::unsupported("merge_continue"))
    }

    async fn merge_abort(&self, repo: &Path) -> GitResult<()> {
        let _ = repo;
        Err(crate::error::GitError::unsupported("merge_abort"))
    }

    async fn rebase(&self, repo: &Path, onto: &str) -> GitResult<crate::types::RebaseResult> {
        let _ = (repo, onto);
        Err(crate::error::GitError::unsupported("rebase"))
    }

    async fn rebase_continue(
        &self,
        repo: &Path,
        message: Option<&str>,
    ) -> GitResult<crate::types::RebaseResult> {
        let _ = (repo, message);
        Err(crate::error::GitError::unsupported("rebase_continue"))
    }

    async fn rebase_skip(&self, repo: &Path) -> GitResult<crate::types::RebaseResult> {
        let _ = repo;
        Err(crate::error::GitError::unsupported("rebase_skip"))
    }

    async fn rebase_abort(&self, repo: &Path) -> GitResult<()> {
        let _ = repo;
        Err(crate::error::GitError::unsupported("rebase_abort"))
    }

    async fn cherry_pick(
        &self,
        repo: &Path,
        commit: &str,
        no_commit: bool,
    ) -> GitResult<crate::types::MergeResult> {
        let _ = (repo, commit, no_commit);
        Err(crate::error::GitError::unsupported("cherry_pick"))
    }

    async fn revert_commit(
        &self,
        repo: &Path,
        commit: &str,
    ) -> GitResult<crate::types::MergeResult> {
        let _ = (repo, commit);
        Err(crate::error::GitError::unsupported("revert_commit"))
    }

    async fn fetch(
        &self,
        repo: &Path,
        remote: &str,
        branch: Option<&str>,
        prune: bool,
    ) -> GitResult<crate::types::FetchResult> {
        let _ = (repo, remote, branch, prune);
        Err(crate::error::GitError::unsupported("fetch"))
    }

    async fn push(
        &self,
        repo: &Path,
        remote: &str,
        branch: &str,
        force: bool,
        set_upstream: bool,
    ) -> GitResult<crate::types::PushResult> {
        let _ = (repo, remote, branch, force, set_upstream);
        Err(crate::error::GitError::unsupported("push"))
    }

    async fn pull(
        &self,
        repo: &Path,
        remote: &str,
        branch: Option<&str>,
    ) -> GitResult<crate::types::PullResult> {
        let _ = (repo, remote, branch);
        Err(crate::error::GitError::unsupported("pull"))
    }

    async fn worktree_list(&self, repo: &Path) -> GitResult<Vec<crate::types::WorktreeInfo>> {
        let _ = repo;
        Err(crate::error::GitError::unsupported("worktree_list"))
    }

    async fn worktree_add(
        &self,
        repo: &Path,
        branch: &str,
        path: &Path,
        create_branch: bool,
    ) -> GitResult<()> {
        let _ = (repo, branch, path, create_branch);
        Err(crate::error::GitError::unsupported("worktree_add"))
    }

    async fn is_linked_worktree(&self, repo: &Path) -> GitResult<bool> {
        let _ = repo;
        Err(crate::error::GitError::unsupported("is_linked_worktree"))
    }

    async fn is_dirty(&self, repo: &Path) -> GitResult<bool> {
        let _ = repo;
        Err(crate::error::GitError::unsupported("is_dirty"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::GitErrorKind;
    use crate::types::{CommitRequest, MergeOptions, StashPushOptions};

    struct Stub;

    #[async_trait]
    impl GitBackend for Stub {
        fn name(&self) -> &'static str {
            "stub"
        }
    }

    fn expect_unsupported<T>(result: crate::error::GitResult<T>) {
        match result {
            Err(e) => assert_eq!(e.kind(), GitErrorKind::Unsupported),
            Ok(_) => panic!("default impl must be Unsupported"),
        }
    }

    #[test]
    fn default_impls_delegate_to_unsupported() {
        let stub = Stub;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let path = Path::new(".");
        let query = LogQuery::default();

        expect_unsupported(rt.block_on(stub.raw(None, &["x"])));
        expect_unsupported(rt.block_on(stub.apply(None, &["x"], "patch")));
        expect_unsupported(rt.block_on(stub.status(path)));
        expect_unsupported(rt.block_on(stub.branches(path, false)));
        expect_unsupported(rt.block_on(stub.diff(path, false, None, 3)));
        expect_unsupported(rt.block_on(stub.remotes(path)));
        expect_unsupported(rt.block_on(stub.in_progress(path)));
        expect_unsupported(rt.block_on(stub.stage_file(path, "x")));
        expect_unsupported(rt.block_on(stub.unstage_file(path, "x")));
        expect_unsupported(rt.block_on(stub.commit(path, CommitRequest::default())));
        expect_unsupported(rt.block_on(stub.stash_list(path)));
        expect_unsupported(rt.block_on(stub.stash_count(path)));
        expect_unsupported(rt.block_on(stub.stash_push(path, StashPushOptions::default())));
        expect_unsupported(rt.block_on(stub.stash_pop(path, 0)));
        expect_unsupported(rt.block_on(stub.stash_apply(path, 0)));
        expect_unsupported(rt.block_on(stub.stash_drop(path, 0)));
        expect_unsupported(rt.block_on(stub.stash_show(path, 0)));
        expect_unsupported(rt.block_on(stub.merge(path, "b", MergeOptions::default())));
        expect_unsupported(rt.block_on(stub.merge_continue(path, None)));
        expect_unsupported(rt.block_on(stub.merge_abort(path)));
        expect_unsupported(rt.block_on(stub.rebase(path, "b")));
        expect_unsupported(rt.block_on(stub.rebase_continue(path, None)));
        expect_unsupported(rt.block_on(stub.rebase_skip(path)));
        expect_unsupported(rt.block_on(stub.rebase_abort(path)));
        expect_unsupported(rt.block_on(stub.cherry_pick(path, "c", false)));
        expect_unsupported(rt.block_on(stub.revert_commit(path, "c")));
        expect_unsupported(rt.block_on(stub.fetch(path, "o", Some("m"), true)));
        expect_unsupported(rt.block_on(stub.push(path, "o", "m", false, false)));
        expect_unsupported(rt.block_on(stub.pull(path, "o", None)));
        expect_unsupported(rt.block_on(stub.commit_files(path, "HEAD")));
        expect_unsupported(rt.block_on(stub.commit_file_diff(path, "HEAD", "f", 3)));
        expect_unsupported(rt.block_on(stub.remote_url(path, "o")));
        expect_unsupported(rt.block_on(stub.config_get(path, "k")));
        expect_unsupported(rt.block_on(stub.config_set(path, "k", "v", false)));
        expect_unsupported(rt.block_on(stub.checkout_branch(path, "b")));
        expect_unsupported(rt.block_on(stub.reset_to_commit(path, "c", "hard")));
        expect_unsupported(rt.block_on(stub.worktree_list(path)));
        expect_unsupported(rt.block_on(stub.worktree_add(path, "b", path, false)));
        expect_unsupported(rt.block_on(stub.is_linked_worktree(path)));
        expect_unsupported(rt.block_on(stub.is_dirty(path)));
        expect_unsupported(rt.block_on(stub.log(path, &query)));
    }
}
