//! CLI backend: single sanctioned owner of `Command::new("git")` in production code.

pub mod parsers;

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::backend::{GitBackend, LogQuery};
use crate::error::{classify_stderr, GitError, GitErrorKind, GitResult};
use crate::types::{
    CommitRequest, CommitResult, GitBranch, GitCommitInfo, GitDiffSummary, GitInProgress,
    GitOperation, GitRemote, GitStashEntry, GitStatus, StashCountFile, StashCountResult,
};

/// Stateless CLI backend; every method takes the repo working directory.
#[derive(Debug, Default, Clone, Copy)]
pub struct CliBackend;

impl CliBackend {
    pub fn new() -> Self {
        Self
    }

    fn classify_failure(args: &[&str], output: &std::process::Output) -> GitError {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if let Some(e) = classify_stderr(&stderr) {
            return e;
        }
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        crate::error::internal_with_output(format!("git {} failed", args.join(" ")), stdout, stderr)
    }

    /// Run git, returning stdout as a string with stderr classified errors.
    pub async fn run_string(&self, workdir: Option<&Path>, args: &[&str]) -> GitResult<String> {
        let output = run_process(workdir, args)
            .await
            .map_err(|e| GitError::from_io("failed to spawn git", e))?;
        if !output.status.success() {
            return Err(Self::classify_failure(args, &output));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Run `git apply`-style commands feeding a patch on stdin.
    pub async fn run_apply(
        &self,
        workdir: Option<&Path>,
        args: &[&str],
        patch: &str,
    ) -> GitResult<()> {
        use tokio::io::AsyncWriteExt;

        let dir = workdir.unwrap_or_else(|| Path::new("."));
        let mut cmd = tokio::process::Command::new("git");
        cmd.args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        apply_no_window(&mut cmd);
        let mut child = cmd
            .spawn()
            .map_err(|e| GitError::from_io("failed to spawn git apply", e))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(patch.as_bytes()).await.ok();
        }
        let output = child
            .wait_with_output()
            .await
            .map_err(|e| GitError::from_io("git apply failed", e))?;

        if !output.status.success() {
            return Err(GitError::invalid_params("patch could not be applied"));
        }
        Ok(())
    }
}

fn apply_no_window(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

/// Spawn git async, returning raw output. Non-zero exit is NOT an error here.
pub async fn run_process(
    workdir: Option<&Path>,
    args: &[&str],
) -> std::io::Result<std::process::Output> {
    let mut cmd = tokio::process::Command::new("git");
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    cmd.args(args);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.output().await
}

/// Sync variant for non-tokio callers (experimental/worktree). Non-zero exit
/// is NOT an error here: callers inspect `Output` directly, as before.
pub fn run_process_sync(workdir: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(workdir).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.output()
}

/// Sync run with classified errors for callers that used the old `run_git`
/// string contract synchronously.
pub fn run_string_sync(workdir: &Path, args: &[&str]) -> GitResult<String> {
    let output =
        run_process_sync(workdir, args).map_err(|e| GitError::from_io("failed to spawn git", e))?;
    if !output.status.success() {
        return Err(CliBackend::classify_failure(args, &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Resolve the repository root (rev-parse --show-toplevel).
pub fn resolve_repo_root(workdir: &Path) -> GitResult<PathBuf> {
    let out = run_string_sync(workdir, &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(out.trim());
    if !root.exists() {
        return Err(GitError::not_found(format!(
            "not a git repository: {}",
            workdir.display()
        )));
    }
    Ok(root)
}

#[async_trait]
impl GitBackend for CliBackend {
    fn name(&self) -> &'static str {
        "cli"
    }

    async fn raw(&self, workdir: Option<&Path>, args: &[&str]) -> GitResult<String> {
        self.run_string(workdir, args).await
    }

    async fn apply(&self, workdir: Option<&Path>, args: &[&str], patch: &str) -> GitResult<()> {
        self.run_apply(workdir, args, patch).await
    }

    async fn status(&self, repo: &Path) -> GitResult<GitStatus> {
        let out = self
            .run_string(Some(repo), &["status", "--porcelain=v2", "--branch"])
            .await?;
        Ok(parsers::parse_porcelain_status_v2(&out))
    }

    async fn log(&self, repo: &Path, query: &LogQuery) -> GitResult<Vec<GitCommitInfo>> {
        let format_arg =
            "--format=%H%x00%P%x00%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI%x00%s%x00%D%x01"
                .to_string();
        let limit_str = query.limit.to_string();
        let skip_str = query.skip.to_string();
        let mut args: Vec<&str> = vec!["log", &format_arg, "-n", &limit_str, "--skip", &skip_str];
        if let Some(ref branch) = query.branch {
            args.push(branch);
        }
        if let Some(ref fp) = query.file_path {
            args.push("--");
            args.push(fp);
        }
        let output = self.run_string(Some(repo), &args).await?;
        Ok(parse_log_output(&output))
    }

    async fn branches(&self, repo: &Path, remote: bool) -> GitResult<Vec<GitBranch>> {
        let format_str =
            "%(refname:short)%00%(HEAD)%00%(upstream:short)%00%(objectname:short)%00%(committerdate:iso)";
        let format_arg = format!("--format={format_str}");
        let local = self
            .run_string(Some(repo), &["for-each-ref", &format_arg, "refs/heads/"])
            .await?;
        let mut output = local;
        if remote {
            let remote_output = self
                .run_string(Some(repo), &["for-each-ref", &format_arg, "refs/remotes/"])
                .await?;
            output.push_str(&remote_output);
        }
        let current_branch = self
            .run_string(Some(repo), &["rev-parse", "--abbrev-ref", "HEAD"])
            .await
            .unwrap_or_default();

        let mut branches = Vec::new();
        for line in output.lines() {
            let parts: Vec<&str> = line.splitn(5, '\0').collect();
            if parts.len() < 5 {
                continue;
            }
            let name = parts[0].to_string();
            let is_current = parts[1] == "*" || name == current_branch.trim();
            let upstream = if parts[2].is_empty() {
                None
            } else {
                Some(parts[2].to_string())
            };
            branches.push(GitBranch {
                is_current,
                is_remote: name.contains('/'),
                name,
                upstream,
                ahead: 0,
                behind: 0,
                last_commit_sha: parts[3].to_string(),
                last_commit_date: parts[4].to_string(),
            });
        }
        Ok(branches)
    }

    async fn diff(
        &self,
        repo: &Path,
        staged: bool,
        path: Option<&str>,
        unified: u32,
    ) -> GitResult<GitDiffSummary> {
        let unified_str = unified.to_string();
        let unified_arg = format!("--unified={unified_str}");
        let mut args: Vec<&str> = vec!["diff", &unified_arg];
        if staged {
            args.push("--cached");
        }
        args.push("--no-color");
        if let Some(p) = path {
            args.push("--");
            args.push(p);
        }
        let diff_text = self.run_string(Some(repo), &args).await?;
        let stat_text = if staged {
            self.run_string(Some(repo), &["diff", "--stat", "--cached"])
                .await?
        } else {
            self.run_string(Some(repo), &["diff", "--stat"]).await?
        };
        Ok(parsers::parse_diff_output(&diff_text, &stat_text))
    }

    async fn remotes(&self, repo: &Path) -> GitResult<Vec<GitRemote>> {
        let output = self.run_string(Some(repo), &["remote", "-v"]).await?;
        let mut seen = std::collections::HashSet::new();
        let mut remotes = Vec::new();
        for line in output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let name = parts[0].to_string();
            if seen.contains(&name) {
                continue;
            }
            seen.insert(name.clone());
            let url = crate::types::sanitize_remote_url(parts[1]);
            let url_type = crate::types::classify_remote_url(&url);
            remotes.push(GitRemote {
                name,
                url,
                url_type,
            });
        }
        Ok(remotes)
    }

    async fn in_progress(&self, repo: &Path) -> GitResult<Option<GitInProgress>> {
        let git_dir = match self
            .run_string(Some(repo), &["rev-parse", "--absolute-git-dir"])
            .await
        {
            Ok(d) => std::path::PathBuf::from(d.trim()),
            Err(_) => return Ok(None),
        };
        let operation =
            if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
                GitOperation::Rebase
            } else if git_dir.join("MERGE_HEAD").exists() {
                GitOperation::Merge
            } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
                GitOperation::CherryPick
            } else if git_dir.join("REVERT_HEAD").exists() {
                GitOperation::Revert
            } else if git_dir.join("BISECT_LOG").exists() {
                GitOperation::Bisect
            } else {
                return Ok(None);
            };
        let conflict_files = self
            .run_string(Some(repo), &["diff", "--name-only", "--diff-filter=U"])
            .await
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>();
        Ok(Some(GitInProgress {
            operation,
            conflict_files,
        }))
    }

    async fn stage_file(&self, repo: &Path, path: &str) -> GitResult<()> {
        self.run_string(Some(repo), &["add", path])
            .await
            .map(|_| ())
    }

    async fn unstage_file(&self, repo: &Path, path: &str) -> GitResult<()> {
        self.run_string(Some(repo), &["restore", "--staged", path])
            .await
            .map(|_| ())
    }

    async fn commit(&self, repo: &Path, req: CommitRequest) -> GitResult<CommitResult> {
        let staged = self
            .run_string(Some(repo), &["diff", "--cached", "--name-only"])
            .await
            .unwrap_or_default();
        if staged.trim().is_empty() && !req.amend {
            return Err(GitError::invalid_params("no staged changes to commit"));
        }

        let mut args: Vec<&str> = vec!["commit", "-m", &req.message];
        if req.amend {
            args.push("--amend");
        }
        if req.signoff {
            args.push("--signoff");
        }
        self.run_string(Some(repo), &args).await?;

        let sha = self
            .run_string(Some(repo), &["rev-parse", "HEAD"])
            .await
            .unwrap_or_default()
            .trim()
            .to_string();
        let branch = self
            .run_string(Some(repo), &["rev-parse", "--abbrev-ref", "HEAD"])
            .await
            .unwrap_or_default()
            .trim()
            .to_string();
        let stat = self
            .run_string(Some(repo), &["show", "--stat", "--format=", "HEAD"])
            .await
            .unwrap_or_default();
        let (insertions, deletions, files_changed) = parsers::parse_commit_stat(&stat);
        Ok(CommitResult {
            sha,
            branch,
            message: req.message.lines().next().unwrap_or("").to_string(),
            files_changed,
            insertions,
            deletions,
            unsigned: None,
            hooks: None,
        })
    }

    async fn stash_list(&self, repo: &Path) -> GitResult<Vec<GitStashEntry>> {
        let output = self
            .run_string(Some(repo), &["stash", "list", "--format=%gd%x00%gs%x00%ci"])
            .await?;
        let mut items = Vec::new();
        for line in output.lines() {
            let parts: Vec<&str> = line.splitn(3, '\0').collect();
            if parts.len() < 3 {
                continue;
            }
            let index_str = parts[0].trim_start_matches("stash@{").trim_end_matches('}');
            items.push(GitStashEntry {
                index: index_str.parse().unwrap_or(0),
                message: parts[1].to_string(),
                date: parts[2].to_string(),
                branch: String::new(),
            });
        }
        Ok(items)
    }

    async fn stash_count(&self, repo: &Path) -> GitResult<StashCountResult> {
        let list = self
            .run_string(Some(repo), &["stash", "list", "--format=%gd"])
            .await?;
        let count = list.lines().filter(|l| !l.is_empty()).count();
        if count == 0 {
            return Ok(StashCountResult {
                count,
                files: Vec::new(),
            });
        }
        let numstat = self
            .run_string(
                Some(repo),
                &["stash", "show", "--numstat", "--format=", "stash@{0}"],
            )
            .await
            .unwrap_or_default();
        let mut files = Vec::new();
        for line in numstat.lines() {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let (ins, del) = if parts[0] == "-" {
                (0, 0)
            } else {
                (parts[0].parse().unwrap_or(0), parts[1].parse().unwrap_or(0))
            };
            let path = parsers::unquote_path(if parts[2].contains(" => ") {
                parts[2].rsplit(" => ").next().unwrap_or(parts[2])
            } else {
                parts[2]
            });
            files.push(StashCountFile {
                path,
                insertions: ins,
                deletions: del,
            });
        }
        Ok(StashCountResult { count, files })
    }
    async fn is_dirty(&self, repo: &Path) -> GitResult<bool> {
        let out = self
            .run_string(Some(repo), &["status", "--porcelain"])
            .await?;
        Ok(!out.trim().is_empty())
    }

    async fn commit_files(
        &self,
        repo: &Path,
        commit: &str,
    ) -> GitResult<Vec<crate::types::CommitFileEntry>> {
        let numstat = self
            .run_string(
                Some(repo),
                &["show", "--numstat", "--format=", "-M", commit],
            )
            .await?;
        let name_status = self
            .run_string(
                Some(repo),
                &["show", "--name-status", "--format=", "-M", commit],
            )
            .await?;
        let mut kinds: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        for line in name_status.lines() {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() < 2 {
                continue;
            }
            let kind = match parts[0].chars().next().unwrap_or('M') {
                'A' => "added",
                'D' => "deleted",
                'R' => "renamed",
                'C' => "copied",
                _ => "modified",
            };
            let path = parsers::unquote_path(parts[1].split('\t').next_back().unwrap_or(parts[1]));
            kinds.insert(path.clone(), kind.to_string());
        }
        let mut out = Vec::new();
        for line in numstat.lines() {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let (ins, del) = if parts[0] == "-" {
                (None, None)
            } else {
                (parts[0].parse::<u32>().ok(), parts[1].parse::<u32>().ok())
            };
            let path = parsers::unquote_path(parts[2].split('\t').next_back().unwrap_or(parts[2]));
            out.push(crate::types::CommitFileEntry {
                path: path.clone(),
                insertions: ins,
                deletions: del,
                status: kinds.get(&path).cloned().unwrap_or_default(),
            });
        }
        Ok(out)
    }

    async fn commit_file_diff(
        &self,
        repo: &Path,
        commit: &str,
        path: &str,
        unified: u32,
    ) -> GitResult<GitDiffSummary> {
        let unified_str = unified.to_string();
        let unified_arg = format!("--unified={unified_str}");
        let diff_text = self
            .run_string(
                Some(repo),
                &["show", &unified_arg, "--no-color", "-M", "--", path, commit],
            )
            .await?;
        let stat_text = self
            .run_string(Some(repo), &["show", "--stat", "--format=", commit])
            .await
            .unwrap_or_default();
        Ok(parsers::parse_diff_output(&diff_text, &stat_text))
    }

    async fn remote_url(&self, repo: &Path, remote: &str) -> GitResult<String> {
        let url = self
            .run_string(Some(repo), &["remote", "get-url", remote])
            .await?;
        let url = url.trim().to_string();
        if url.is_empty() {
            return Err(GitError::not_found(format!("remote '{remote}' not found")));
        }
        Ok(url)
    }

    async fn config_get(&self, repo: &Path, key: &str) -> GitResult<Option<String>> {
        match self.run_string(Some(repo), &["config", "--get", key]).await {
            Ok(v) if v.trim().is_empty() => Ok(None),
            Ok(v) => Ok(Some(v.trim_end_matches('\n').to_string())),
            Err(_) => Ok(None),
        }
    }

    async fn config_set(&self, repo: &Path, key: &str, value: &str, global: bool) -> GitResult<()> {
        if global {
            self.run_string(Some(repo), &["config", "--global", key, value])
                .await
                .map(|_| ())
        } else {
            self.run_string(Some(repo), &["config", key, value])
                .await
                .map(|_| ())
        }
    }

    async fn checkout_branch(&self, repo: &Path, branch: &str) -> GitResult<String> {
        if self
            .run_string(Some(repo), &["checkout", branch])
            .await
            .is_ok()
        {
            return Ok(branch.to_string());
        }
        let remote_ref = format!("origin/{branch}");
        let out = self
            .run_string(Some(repo), &["checkout", &remote_ref])
            .await
            .map_err(|_| GitError::not_found(format!("branch '{branch}' not found")))?;
        let new_branch = out
            .lines()
            .find_map(|l| l.split("into '").nth(1))
            .map(|s| s.trim_end_matches('\'').to_string())
            .unwrap_or_else(|| branch.to_string());
        Ok(new_branch)
    }

    async fn reset_to_commit(&self, repo: &Path, commit: &str, mode: &str) -> GitResult<()> {
        match mode {
            "soft" | "mixed" | "hard" => {}
            other => {
                return Err(GitError::invalid_params(format!(
                    "unknown reset mode '{other}'"
                )))
            }
        }
        let flag = format!("--{mode}");
        self.run_string(Some(repo), &["reset", &flag, commit])
            .await
            .map(|_| ())
    }

    async fn stage_hunk(&self, repo: &Path, patch: &str) -> GitResult<()> {
        self.run_apply(
            Some(repo),
            &["apply", "--cached", "--unidiff-zero", "-"],
            patch,
        )
        .await
    }

    async fn unstage_hunk(&self, repo: &Path, patch: &str) -> GitResult<()> {
        self.run_apply(
            Some(repo),
            &["apply", "--cached", "--reverse", "--unidiff-zero", "-"],
            patch,
        )
        .await
    }

    async fn revert_hunk(&self, repo: &Path, patch: &str) -> GitResult<()> {
        self.run_apply(
            Some(repo),
            &["apply", "--reverse", "--unidiff-zero", "-"],
            patch,
        )
        .await
    }

    async fn stash_push(
        &self,
        repo: &Path,
        opts: crate::types::StashPushOptions,
    ) -> GitResult<bool> {
        let mut args: Vec<String> = vec!["stash".into(), "push".into()];
        if opts.include_untracked {
            args.push("--include-untracked".into());
        }
        if opts.keep_index {
            args.push("--keep-index".into());
        }
        if let Some(m) = &opts.message {
            args.push("-m".into());
            args.push(m.clone());
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let out = self.run_string(Some(repo), &arg_refs).await?;
        Ok(out.contains("Saved working directory"))
    }

    async fn stash_pop(&self, repo: &Path, index: usize) -> GitResult<()> {
        let target = format!("stash@{{{index}}}");
        self.run_string(Some(repo), &["stash", "pop", &target])
            .await
            .map(|_| ())
    }

    async fn stash_apply(&self, repo: &Path, index: usize) -> GitResult<()> {
        let target = format!("stash@{{{index}}}");
        self.run_string(Some(repo), &["stash", "apply", &target])
            .await
            .map(|_| ())
    }

    async fn stash_drop(&self, repo: &Path, index: usize) -> GitResult<()> {
        let target = format!("stash@{{{index}}}");
        self.run_string(Some(repo), &["stash", "drop", &target])
            .await
            .map(|_| ())
    }

    async fn stash_show(
        &self,
        repo: &Path,
        index: usize,
    ) -> GitResult<crate::types::StashCountResult> {
        let target = format!("stash@{{{index}}}");
        let numstat = self
            .run_string(
                Some(repo),
                &["stash", "show", "--numstat", "--format=", &target],
            )
            .await?;
        let count = self
            .run_string(Some(repo), &["stash", "list", "--format=%gd"])
            .await?
            .lines()
            .filter(|l| !l.is_empty())
            .count();
        let mut files = Vec::new();
        for line in numstat.lines() {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let (ins, del) = if parts[0] == "-" {
                (0, 0)
            } else {
                (parts[0].parse().unwrap_or(0), parts[1].parse().unwrap_or(0))
            };
            let path = parsers::unquote_path(parts[2].rsplit(" => ").next().unwrap_or(parts[2]));
            files.push(StashCountFile {
                path,
                insertions: ins,
                deletions: del,
            });
        }
        Ok(StashCountResult { count, files })
    }

    async fn merge(
        &self,
        repo: &Path,
        branch: &str,
        opts: crate::types::MergeOptions,
    ) -> GitResult<crate::types::MergeResult> {
        let fast_forward_candidate = self
            .run_string(Some(repo), &["merge-base", "--is-ancestor", "HEAD", branch])
            .await
            .is_ok()
            && !opts.no_ff
            && !opts.squash;

        let mut args: Vec<&str> = vec!["merge", branch];
        if opts.no_ff && !opts.squash {
            args.push("--no-ff");
        }
        if opts.squash {
            args.push("--squash");
        }
        if let Some(m) = &opts.message {
            args.push("-m");
            args.push(m);
        }
        match self.run_string(Some(repo), &args).await {
            Ok(_) => {
                let merge_commit = if opts.squash || fast_forward_candidate {
                    None
                } else {
                    self.run_string(Some(repo), &["rev-parse", "HEAD"])
                        .await
                        .ok()
                        .map(|s| s.trim().to_string())
                };
                Ok(crate::types::MergeResult {
                    fast_forward: fast_forward_candidate,
                    merge_commit,
                    conflicts: vec![],
                    conflicted: false,
                    squashed: opts.squash,
                })
            }
            Err(e) => {
                let is_conflict = e.kind() == GitErrorKind::Conflict
                    || e.stderr().unwrap_or("").contains("CONFLICT")
                    || e.stdout().unwrap_or("").contains("CONFLICT");
                if is_conflict {
                    let conflicts = self
                        .run_string(Some(repo), &["diff", "--name-only", "--diff-filter=U"])
                        .await
                        .unwrap_or_default()
                        .lines()
                        .filter(|l| !l.is_empty())
                        .map(|l| l.trim().to_string())
                        .collect();
                    Ok(crate::types::MergeResult {
                        fast_forward: false,
                        merge_commit: None,
                        conflicts,
                        conflicted: true,
                        squashed: false,
                    })
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn merge_continue(
        &self,
        repo: &Path,
        message: Option<&str>,
    ) -> GitResult<crate::types::MergeResult> {
        let conflicts = self
            .run_string(Some(repo), &["diff", "--name-only", "--diff-filter=U"])
            .await
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count();
        if conflicts > 0 {
            return Err(GitError::conflict("unresolved merge conflicts"));
        }
        let commit_res = match message {
            Some(m) => self.run_string(Some(repo), &["commit", "-m", m]).await,
            None => self.run_string(Some(repo), &["commit", "--no-edit"]).await,
        };
        commit_res.map_err(|e| {
            if e.message().contains("no merge in progress") {
                GitError::conflict("no merge in progress")
            } else {
                e
            }
        })?;
        let sha = self
            .run_string(Some(repo), &["rev-parse", "HEAD"])
            .await?
            .trim()
            .to_string();
        Ok(crate::types::MergeResult {
            fast_forward: false,
            merge_commit: Some(sha),
            conflicts: vec![],
            conflicted: false,
            squashed: false,
        })
    }

    async fn merge_abort(&self, repo: &Path) -> GitResult<()> {
        self.run_string(Some(repo), &["merge", "--abort"])
            .await
            .map(|_| ())
    }

    async fn rebase(&self, repo: &Path, onto: &str) -> GitResult<crate::types::RebaseResult> {
        self.run_rebase_cmd(repo, &["rebase", onto]).await
    }

    async fn rebase_continue(
        &self,
        repo: &Path,
        _message: Option<&str>,
    ) -> GitResult<crate::types::RebaseResult> {
        self.run_rebase_cmd(repo, &["rebase", "--continue"]).await
    }

    async fn rebase_skip(&self, repo: &Path) -> GitResult<crate::types::RebaseResult> {
        self.run_rebase_cmd(repo, &["rebase", "--skip"]).await
    }

    async fn rebase_abort(&self, repo: &Path) -> GitResult<()> {
        self.run_rebase_cmd(repo, &["rebase", "--abort"])
            .await
            .map(|_| ())
    }

    async fn cherry_pick(
        &self,
        repo: &Path,
        commit: &str,
        no_commit: bool,
    ) -> GitResult<crate::types::MergeResult> {
        let mut args: Vec<&str> = vec!["cherry-pick"];
        if no_commit {
            args.push("--no-commit");
        }
        args.push(commit);
        let conflicts = self
            .run_string(Some(repo), &["diff", "--name-only", "--diff-filter=U"])
            .await
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<String>>();
        match self.run_string(Some(repo), &args).await {
            Ok(_) => {}
            Err(e) => {
                if !conflicts.is_empty() {
                    return Ok(crate::types::MergeResult {
                        conflicts,
                        conflicted: true,
                        ..Default::default()
                    });
                }
                return Err(e);
            }
        }
        let sha = if no_commit {
            None
        } else {
            self.run_string(Some(repo), &["rev-parse", "HEAD"])
                .await
                .ok()
                .map(|s| s.trim().to_string())
        };
        Ok(crate::types::MergeResult {
            merge_commit: sha,
            conflicts: vec![],
            conflicted: false,
            squashed: false,
            fast_forward: false,
        })
    }

    async fn revert_commit(
        &self,
        repo: &Path,
        commit: &str,
    ) -> GitResult<crate::types::MergeResult> {
        match self
            .run_string(Some(repo), &["revert", "--no-edit", commit])
            .await
        {
            Ok(_) => {}
            Err(e) => {
                let conflicts = self
                    .run_string(Some(repo), &["diff", "--name-only", "--diff-filter=U"])
                    .await
                    .unwrap_or_default()
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<String>>();
                if !conflicts.is_empty() {
                    return Ok(crate::types::MergeResult {
                        conflicts,
                        conflicted: true,
                        ..Default::default()
                    });
                }
                return Err(e);
            }
        }
        let sha = self
            .run_string(Some(repo), &["rev-parse", "HEAD"])
            .await?
            .trim()
            .to_string();
        Ok(crate::types::MergeResult {
            merge_commit: Some(sha),
            ..Default::default()
        })
    }

    async fn fetch(
        &self,
        repo: &Path,
        remote: &str,
        branch: Option<&str>,
        prune: bool,
    ) -> GitResult<crate::types::FetchResult> {
        let mut args: Vec<String> = vec!["fetch".into()];
        if prune {
            args.push("--prune".into());
        }
        args.push(remote.to_string());
        if let Some(b) = branch {
            args.push(b.to_string());
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.run_string(Some(repo), &arg_refs).await?;
        let git_dir = self
            .run_string(Some(repo), &["rev-parse", "--absolute-git-dir"])
            .await
            .unwrap_or_else(|_| ".git".into())
            .trim()
            .to_string();
        let fetched = std::fs::read_to_string(std::path::Path::new(&git_dir).join("FETCH_HEAD"))
            .unwrap_or_default();
        let mut refs = Vec::new();
        for line in fetched.lines() {
            // FETCH_HEAD rows are: <sha>\t\tbranch 'main' of <url>
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() < 3 || parts[0].is_empty() {
                continue;
            }
            let sha = parts[0];
            let desc = parts[2];
            let branch_part = desc
                .split(" of ")
                .next()
                .unwrap_or(desc)
                .trim_matches(|c| c == '\'' || c == ' ');
            refs.push(crate::types::FetchedRef {
                ref_name: branch_part.to_string(),
                old_sha: String::new(),
                new_sha: sha.to_string(),
            });
        }
        Ok(crate::types::FetchResult {
            fetched_refs: refs,
            fetch_via: None,
        })
    }

    async fn push(
        &self,
        repo: &Path,
        remote: &str,
        branch: &str,
        force: bool,
        set_upstream: bool,
    ) -> GitResult<crate::types::PushResult> {
        let mut args: Vec<String> = vec!["push".into()];
        if force {
            // §B3-7c force-with-lease: refresh the remote-tracking ref first so
            // the lease compares against the actual remote tip.
            self.run_string(Some(repo), &["fetch", remote]).await.ok();
            args.push("--force-with-lease".into());
        }
        if set_upstream {
            args.push("-u".into());
        }
        args.push(remote.to_string());
        args.push(branch.to_string());
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.run_string(Some(repo), &arg_refs).await?;
        let remote_sha = self
            .run_string(Some(repo), &["rev-parse", &format!("{remote}/{branch}")])
            .await
            .unwrap_or_default()
            .trim()
            .to_string();
        Ok(crate::types::PushResult {
            remote_sha,
            push_via: None,
        })
    }

    async fn pull(
        &self,
        repo: &Path,
        remote: &str,
        branch: Option<&str>,
    ) -> GitResult<crate::types::PullResult> {
        let mut args: Vec<String> = vec!["pull".into(), remote.to_string()];
        if let Some(b) = branch {
            args.push(b.to_string());
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let output = self.run_string(Some(repo), &arg_refs).await?;
        let fast_forward = output.contains("Fast-forward");
        let conflicts = self
            .run_string(Some(repo), &["diff", "--name-only", "--diff-filter=U"])
            .await
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<String>>();
        let merge_commit = if fast_forward || !conflicts.is_empty() {
            None
        } else {
            self.run_string(Some(repo), &["rev-parse", "HEAD"])
                .await
                .ok()
                .map(|s| s.trim().to_string())
        };
        Ok(crate::types::PullResult {
            fast_forward,
            merge_commit,
            conflicts,
        })
    }

    async fn worktree_list(&self, repo: &Path) -> GitResult<Vec<crate::types::WorktreeInfo>> {
        let output = self
            .run_string(Some(repo), &["worktree", "list", "--porcelain"])
            .await?;
        let mut out = Vec::new();
        let mut current = crate::types::WorktreeInfo {
            name: String::new(),
            path: String::new(),
            branch: None,
            head: None,
        };
        for line in output.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                if !current.path.is_empty() {
                    out.push(current.clone());
                }
                let path = p.to_string();
                current = crate::types::WorktreeInfo {
                    name: std::path::Path::new(&path)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    path,
                    branch: None,
                    head: None,
                };
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                current.head = Some(h.to_string());
            } else if let Some(b) = line.strip_prefix("branch ") {
                let name = b.trim_start_matches("refs/heads/").to_string();
                current.branch = Some(name.clone());
                if current.name.is_empty() {
                    current.name = name;
                }
            }
        }
        if !current.path.is_empty() {
            out.push(current);
        }
        Ok(out)
    }

    async fn worktree_add(
        &self,
        repo: &Path,
        branch: &str,
        path: &Path,
        create_branch: bool,
    ) -> GitResult<()> {
        let path_str = path.to_string_lossy().into_owned();
        let branch_arg = branch.to_string();
        let result = if create_branch {
            self.run_string(
                Some(repo),
                &["worktree", "add", "-b", &branch_arg, &path_str],
            )
            .await
        } else {
            self.run_string(Some(repo), &["worktree", "add", &path_str, &branch_arg])
                .await
        };
        result.map(|_| ()).map_err(|e| {
            if e.message().contains("already exists") {
                GitError::conflict(e.message().to_string())
            } else {
                e
            }
        })
    }

    async fn is_linked_worktree(&self, repo: &Path) -> GitResult<bool> {
        let common = self
            .run_string(
                Some(repo),
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await
            .unwrap_or_default();
        let own = self
            .run_string(Some(repo), &["rev-parse", "--absolute-git-dir"])
            .await
            .unwrap_or_default();
        Ok(!common.trim().is_empty()
            && !own.trim().is_empty()
            && !common.trim().eq_ignore_ascii_case(own.trim()))
    }
}

impl CliBackend {
    /// Run a rebase-family command with editors neutralized (bug#1: `-i`
    /// hung waiting on an editor; GIT_SEQUENCE_EDITOR/GIT_EDITOR `:` keeps
    /// every code path non-interactive).
    async fn run_rebase_cmd(
        &self,
        repo: &Path,
        args: &[&str],
    ) -> GitResult<crate::types::RebaseResult> {
        let mut cmd = tokio::process::Command::new("git");
        cmd.args(args)
            .current_dir(repo)
            .env("GIT_SEQUENCE_EDITOR", ":")
            .env("GIT_EDITOR", ":")
            .env("EDITOR", ":")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let child = cmd
            .spawn()
            .map_err(|e| GitError::from_io("failed to spawn git rebase", e))?;
        let output = child
            .wait_with_output()
            .await
            .map_err(|e| GitError::from_io("git rebase failed", e))?;

        let conflicts = self
            .run_string(Some(repo), &["diff", "--name-only", "--diff-filter=U"])
            .await
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>();

        if !output.status.success() {
            if !conflicts.is_empty() {
                return Ok(crate::types::RebaseResult {
                    conflicts,
                    conflicted: true,
                    completed: false,
                });
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::internal(stderr.to_string()));
        }
        Ok(crate::types::RebaseResult {
            conflicts: vec![],
            conflicted: false,
            completed: !args.contains(&"--abort"),
        })
    }
}

/// Parse `git log` output using the NUL×10 + \x01 separator format.
pub fn parse_log_output(output: &str) -> Vec<GitCommitInfo> {
    let mut items = Vec::new();
    for entry in output.split('\x01') {
        let entry = entry.trim_start_matches('\n');
        if entry.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = entry.splitn(10, '\0').collect();
        if parts.len() < 10 {
            continue;
        }
        items.push(GitCommitInfo {
            sha: parts[0].to_string(),
            parents: parts[1].split_whitespace().map(|s| s.to_string()).collect(),
            author: parts[2].to_string(),
            author_email: parts[3].to_string(),
            author_date: parts[4].to_string(),
            committer: parts[5].to_string(),
            committer_email: parts[6].to_string(),
            committer_date: parts[7].to_string(),
            message: parts[8].trim_end().to_string(),
            refs: parts[9]
                .split(", ")
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect(),
        });
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_output_parse() {
        let out = "abc123\x00\x00Alice\x00a@x\x002025-01-01\x00Bob\x00b@x\x002025-01-02\x00msg\x00HEAD -> main\x01";
        let items = parse_log_output(out);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].sha, "abc123");
        assert_eq!(items[0].message, "msg");
        assert_eq!(items[0].refs, vec!["HEAD -> main".to_string()]);
    }
}
