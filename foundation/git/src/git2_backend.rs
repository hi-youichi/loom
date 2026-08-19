//! git2 backend: read paths (status/log/branches/diff/remotes), staging,
//! commit, stash reads, and state detection. Methods not implemented here
//! return `Unsupported` and the facade delegates them to CliBackend.

use std::collections::HashMap;
use std::path::Path;

use async_trait::async_trait;
use git2::{Repository, RepositoryState, Sort};

use crate::backend::{GitBackend, LogQuery};
use crate::error::{GitError, GitErrorKind, GitResult};
use crate::types::{
    classify_remote_url, sanitize_remote_url, CommitRequest, CommitResult, GitBranch,
    GitCommitInfo, GitDiffHunk, GitDiffLine, GitDiffLineKind, GitDiffStat, GitDiffSummary,
    GitFileStatus, GitInProgress, GitOperation, GitRemote, GitStashEntry, GitStatus, GitStatusFile,
    StashCountFile, StashCountResult,
};

/// Map a git2 error into the facade's classified error.
pub(crate) fn map_err(e: git2::Error) -> GitError {
    let kind = match e.code() {
        git2::ErrorCode::NotFound => GitErrorKind::NotFound,
        git2::ErrorCode::Exists => GitErrorKind::Conflict,
        _ if e.message().to_lowercase().contains("lock") => GitErrorKind::Locked,
        _ => GitErrorKind::Internal,
    };
    GitError::new(kind, e.message().to_string())
}

fn open_repo(dir: &Path) -> GitResult<Repository> {
    Repository::discover(dir).map_err(|e| {
        if e.code() == git2::ErrorCode::NotFound {
            GitError::not_found("not a git repository")
        } else {
            map_err(e)
        }
    })
}

#[cfg_attr(coverage, coverage(off))]
async fn blocking<T, F>(dir: &Path, f: F) -> GitResult<T>
where
    T: Send + 'static,
    F: FnOnce(Repository) -> GitResult<T> + Send + 'static,
{
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let repo = open_repo(&dir)?;
        f(repo)
    })
    .await
    .map_err(|e| GitError::internal(format!("git task join error: {e}")))?
}

#[cfg_attr(coverage, coverage(off))]
fn iso8601_strict(t: git2::Time) -> String {
    let secs = t.seconds();
    let off = t.offset_minutes();
    let (h, m) = if off < 0 {
        (-(off / 60), -(off % 60))
    } else {
        (off / 60, off % 60)
    };
    let offset = time::UtcOffset::from_hms(h as i8, m as i8, 0).unwrap_or(time::UtcOffset::UTC);
    let dt = time::OffsetDateTime::from_unix_timestamp(secs)
        .map(|d| d.to_offset(offset))
        .ok();
    let Some(dt) = dt else {
        return String::new();
    };
    let fmt = time::macros::format_description!(
        "[year]-[month]-[day]T[hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_minute]"
    );
    dt.format(&fmt).unwrap_or_default()
}

#[cfg_attr(coverage, coverage(off))]
fn iso_date_for_branch(t: git2::Time) -> String {
    let secs = t.seconds();
    let off = t.offset_minutes();
    let (h, m) = if off < 0 {
        (-(off / 60), -(off % 60))
    } else {
        (off / 60, off % 60)
    };
    let offset = time::UtcOffset::from_hms(h as i8, m as i8, 0).unwrap_or(time::UtcOffset::UTC);
    let dt = time::OffsetDateTime::from_unix_timestamp(secs)
        .map(|d| d.to_offset(offset))
        .ok();
    let Some(dt) = dt else {
        return String::new();
    };
    let fmt = time::macros::format_description!(
        "[year]-[month]-[day] [hour]:[minute]:[second] [offset_hour sign:mandatory][offset_minute]"
    );
    dt.format(&fmt).unwrap_or_default()
}

fn state_to_operation(state: RepositoryState) -> Option<GitOperation> {
    match state {
        RepositoryState::Clean => None,
        RepositoryState::Merge => Some(GitOperation::Merge),
        RepositoryState::Rebase
        | RepositoryState::RebaseInteractive
        | RepositoryState::RebaseMerge => Some(GitOperation::Rebase),
        RepositoryState::CherryPick | RepositoryState::CherryPickSequence => {
            Some(GitOperation::CherryPick)
        }
        RepositoryState::Revert | RepositoryState::RevertSequence => Some(GitOperation::Revert),
        RepositoryState::ApplyMailbox | RepositoryState::ApplyMailboxOrRebase => {
            Some(GitOperation::Rebase)
        }
        RepositoryState::Bisect => Some(GitOperation::Bisect),
    }
}

fn index_status_from_flags(st: git2::Status) -> GitFileStatus {
    if st.is_conflicted() {
        return GitFileStatus::Unmerged;
    }
    if st.is_index_new() {
        return GitFileStatus::Added;
    }
    if st.is_index_modified() {
        return GitFileStatus::Modified;
    }
    if st.is_index_deleted() {
        return GitFileStatus::Deleted;
    }
    if st.is_index_renamed() {
        return GitFileStatus::Renamed;
    }
    if st.is_index_typechange() {
        return GitFileStatus::Modified;
    }
    GitFileStatus::Unmodified
}

fn working_status_from_flags(st: git2::Status) -> GitFileStatus {
    if st.is_conflicted() {
        return GitFileStatus::Unmerged;
    }
    if st.is_wt_new() {
        return GitFileStatus::Untracked;
    }
    if st.is_wt_modified() {
        return GitFileStatus::Modified;
    }
    if st.is_wt_deleted() {
        return GitFileStatus::Deleted;
    }
    if st.is_wt_renamed() {
        return GitFileStatus::Renamed;
    }
    if st.is_wt_typechange() {
        return GitFileStatus::Modified;
    }
    GitFileStatus::Unmodified
}

fn status_blocking(repo: &Repository, include_operation: bool) -> GitResult<GitStatus> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .update_index(true);

    let statuses = repo.statuses(Some(&mut opts)).map_err(map_err)?;

    let mut files = Vec::new();
    let mut conflict_files = Vec::new();
    for entry in statuses.iter() {
        let st = entry.status();
        let path = entry
            .index_to_workdir()
            .or_else(|| entry.head_to_index())
            .and_then(|d| {
                d.new_file()
                    .path()
                    .map(|p| p.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        if path.is_empty() {
            continue;
        }
        if st.is_conflicted() {
            conflict_files.push(path.clone());
        }
        files.push(GitStatusFile {
            path,
            index_status: index_status_from_flags(st),
            working_status: working_status_from_flags(st),
        });
    }
    for f in files.iter_mut() {
        // porcelain v2 renders untracked rows as "? path" — both columns '?'.
        if matches!(f.working_status, GitFileStatus::Untracked) {
            f.index_status = GitFileStatus::Untracked;
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    conflict_files.sort();

    let (branch, upstream, ahead, behind) = branch_tracking(repo);

    let in_progress = if include_operation {
        if let Some(op) = state_to_operation(repo.state()) {
            Some(GitInProgress {
                operation: op,
                conflict_files: conflict_files.clone(),
            })
        } else if !conflict_files.is_empty() {
            Some(GitInProgress {
                operation: GitOperation::Merge,
                conflict_files,
            })
        } else {
            None
        }
    } else {
        None
    };

    Ok(GitStatus {
        branch,
        upstream,
        ahead,
        behind,
        files,
        in_progress,
    })
}

/// (branch, upstream, ahead, behind) from HEAD + its tracking ref.
pub(crate) fn branch_tracking(repo: &Repository) -> (String, Option<String>, u32, u32) {
    if repo.head_detached().unwrap_or(false) {
        return ("(detached)".to_string(), None, 0, 0);
    }
    let Ok(head) = repo.head() else {
        return ("(unborn)".to_string(), None, 0, 0);
    };
    let branch = head.shorthand().unwrap_or("HEAD").to_string();

    let upstream = repo
        .find_branch(&branch, git2::BranchType::Local)
        .ok()
        .and_then(|b| b.upstream().ok())
        .and_then(|u| u.name().ok().flatten().map(|s| s.to_string()));

    let mut ahead = 0;
    let mut behind = 0;
    if let (Some(local_oid), Some(name)) = (head.target(), upstream.clone()) {
        if let Some(up_oid) = repo
            .find_reference(&format!("refs/remotes/{name}"))
            .ok()
            .and_then(|r| r.target())
        {
            if let Ok((a, b)) = repo.graph_ahead_behind(local_oid, up_oid) {
                ahead = a as u32;
                behind = b as u32;
            }
        }
    }
    (branch, upstream, ahead, behind)
}

/// Map of commit oid -> %D-style decorations, for `log`.
fn decorations(repo: &Repository) -> HashMap<git2::Oid, Vec<String>> {
    let mut map: HashMap<git2::Oid, Vec<String>> = HashMap::new();
    let head_branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()));
    let Ok(refs) = repo.references() else {
        return map;
    };
    for r in refs.flatten() {
        let Some(oid) = r.target() else { continue };
        let Some(name) = r.shorthand() else {
            continue;
        };
        let label = if r.is_tag() {
            format!("tag: {name}")
        } else {
            name.to_string()
        };
        map.entry(oid).or_default().push(label);
    }
    if let (Some(branch), Ok(head)) = (&head_branch, repo.head()) {
        if let Some(oid) = head.target() {
            if let Some(labels) = map.get_mut(&oid) {
                let idx = labels.iter().position(|l| l == branch);
                if let Some(i) = idx {
                    labels[i] = format!("HEAD -> {branch}");
                } else {
                    labels.insert(0, format!("HEAD -> {branch}"));
                }
            } else {
                map.insert(oid, vec![format!("HEAD -> {branch}")]);
            }
        }
    }
    map
}

fn commit_info(
    commit: &git2::Commit,
    deco: &HashMap<git2::Oid, Vec<String>>,
) -> GitResult<GitCommitInfo> {
    Ok(GitCommitInfo {
        sha: commit.id().to_string(),
        parents: commit.parent_ids().map(|p| p.to_string()).collect(),
        author: commit.author().name().unwrap_or("").to_string(),
        author_email: commit.author().email().unwrap_or("").to_string(),
        author_date: iso8601_strict(commit.author().when()),
        committer: commit.committer().name().unwrap_or("").to_string(),
        committer_email: commit.committer().email().unwrap_or("").to_string(),
        committer_date: iso8601_strict(commit.committer().when()),
        message: commit.summary().unwrap_or_default().to_string(),
        refs: deco.get(&commit.id()).cloned().unwrap_or_default(),
    })
}

fn tree_of<'a>(commit: &'a git2::Commit<'a>) -> GitResult<git2::Tree<'a>> {
    commit.tree().map_err(map_err)
}

fn commit_touches_path(repo: &Repository, commit: &git2::Commit, path: &str) -> GitResult<bool> {
    let cur = tree_of(commit)?;
    let parent_count = commit.parent_count();
    let mut any = false;
    for i in 0..parent_count {
        let ptree = commit.parent(i).ok().and_then(|p| p.tree().ok());
        let diff = repo
            .diff_tree_to_tree(ptree.as_ref(), Some(&cur), None)
            .map_err(map_err)?;
        if diff.deltas().any(|d| {
            d.old_file()
                .path()
                .map(|p| p.to_string_lossy() == path)
                .unwrap_or(false)
                || d.new_file()
                    .path()
                    .map(|p| p.to_string_lossy() == path)
                    .unwrap_or(false)
        }) {
            any = true;
            break;
        }
    }
    if parent_count == 0 {
        let empty_tree_id = repo
            .treebuilder(None)
            .and_then(|tb| tb.write())
            .map_err(map_err)?;
        let empty = repo.find_tree(empty_tree_id).map_err(map_err)?;
        let diff = repo
            .diff_tree_to_tree(Some(&empty), Some(&cur), None)
            .map_err(map_err)?;
        any = diff.deltas().any(|d| {
            d.new_file()
                .path()
                .map(|p| p.to_string_lossy() == path)
                .unwrap_or(false)
        });
    }
    Ok(any)
}

fn log_blocking(repo: &Repository, query: &LogQuery) -> GitResult<Vec<GitCommitInfo>> {
    let mut walk = repo.revwalk().map_err(map_err)?;
    if let Some(branch) = &query.branch {
        let oid = repo
            .revparse_single(branch)
            .map_err(|e| {
                let msg = e.message();
                if msg.contains("unknown revision") || msg.contains("does not exist") {
                    GitError::not_found(msg)
                } else {
                    map_err(e)
                }
            })?
            .id();
        walk.push(oid).map_err(map_err)?;
    } else {
        walk.push_head().map_err(|e| {
            let msg = e.message();
            if e.code() == git2::ErrorCode::NotFound
                || msg.contains("not found")
                || msg.contains("does not point")
                || msg.contains("unborn")
            {
                GitError::not_found("repository does not have any commits yet")
            } else {
                map_err(e)
            }
        })?;
    }
    walk.set_sorting(Sort::TIME | Sort::TOPOLOGICAL)
        .map_err(map_err)?;

    let deco = decorations(repo);
    let mut items = Vec::new();
    for (n, oid) in walk.flatten().enumerate() {
        if n < query.skip {
            continue;
        }
        if items.len() >= query.limit {
            break;
        }
        let commit = match repo.find_commit(oid) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Some(path) = &query.file_path {
            if !commit_touches_path(repo, &commit, path)? {
                continue;
            }
        }
        items.push(commit_info(&commit, &deco)?);
    }
    Ok(items)
}

fn branch_info(
    repo: &Repository,
    current_branch: &str,
    name: &str,
    is_remote: bool,
    oid: git2::Oid,
    committer: git2::Time,
) -> GitBranch {
    let upstream = if is_remote {
        None
    } else {
        repo.find_branch(name, git2::BranchType::Local)
            .ok()
            .and_then(|b| b.upstream().ok())
            .and_then(|u| u.name().ok().flatten().map(|s| s.to_string()))
    };
    GitBranch {
        name: name.to_string(),
        is_current: !is_remote && name == current_branch,
        is_remote,
        upstream,
        ahead: 0,
        behind: 0,
        last_commit_sha: oid.to_string().chars().take(7).collect(),
        last_commit_date: iso_date_for_branch(committer),
    }
}

fn branches_blocking(repo: &Repository, remote: bool) -> GitResult<Vec<GitBranch>> {
    let current_branch = if repo.head_detached().unwrap_or(false) {
        String::new()
    } else {
        repo.head()
            .ok()
            .and_then(|h| h.shorthand().map(|s| s.to_string()))
            .unwrap_or_default()
    };

    let mut branches = Vec::new();
    let Ok(refs) = repo.references() else {
        return Ok(branches);
    };
    for r in refs.flatten() {
        let Some(name) = r.shorthand() else { continue };
        let Some(full) = r.name() else { continue };
        let Some(oid) = r.target() else { continue };
        let is_remote = full.starts_with("refs/remotes/");
        if full.starts_with("refs/heads/") {
            let committer = repo
                .find_commit(oid)
                .map(|c| c.committer().when())
                .unwrap_or_else(|_| git2::Time::new(0, 0));
            branches.push(branch_info(
                repo,
                &current_branch,
                name,
                false,
                oid,
                committer,
            ));
        } else if remote && is_remote {
            let committer = repo
                .find_commit(oid)
                .map(|c| c.committer().when())
                .unwrap_or_else(|_| git2::Time::new(0, 0));
            branches.push(branch_info(
                repo,
                &current_branch,
                name,
                true,
                oid,
                committer,
            ));
        }
    }
    branches.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(branches)
}

fn diff_blocking(
    repo: &Repository,
    staged: bool,
    path: Option<&str>,
    unified: u32,
) -> GitResult<GitDiffSummary> {
    let mut opts = git2::DiffOptions::new();
    opts.context_lines(unified);
    if let Some(p) = path {
        opts.pathspec(p);
    }

    let diff = if staged {
        let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
        let mut index = repo.index().map_err(map_err)?;
        let tree_id = index.write_tree().map_err(map_err)?;
        let index_tree = repo.find_tree(tree_id).map_err(map_err)?;
        repo.diff_tree_to_tree(head_tree.as_ref(), Some(&index_tree), Some(&mut opts))
    } else {
        let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
        repo.diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts))
    }
    .map_err(map_err)?;

    let mut diff = diff;
    diff.find_similar(None).ok();

    collect_diff_summary(&diff)
}

fn collect_diff_summary(diff: &git2::Diff) -> GitResult<GitDiffSummary> {
    collect_diff_summary_pub(diff)
}

pub(super) fn collect_diff_summary_pub(diff: &git2::Diff) -> GitResult<GitDiffSummary> {
    let stats = diff.stats().map_err(map_err)?;
    let files_changed = stats.files_changed() as u32;
    let insertions = stats.insertions() as u32;
    let deletions = stats.deletions() as u32;

    let hunks: std::sync::Mutex<Vec<GitDiffHunk>> = std::sync::Mutex::new(Vec::new());
    let cur: std::sync::Mutex<Option<GitDiffHunk>> = std::sync::Mutex::new(None);

    {
        let mut file_cb = |_d: git2::DiffDelta, _f: f32| -> bool { true };
        let mut binary_cb = |_d: git2::DiffDelta, _b: git2::DiffBinary| -> bool { true };
        let mut hunk_cb = |d: git2::DiffDelta, h: git2::DiffHunk| -> bool {
            if let Ok(mut hk) = cur.lock() {
                if let Some(prev) = hk.take() {
                    if let Ok(mut all) = hunks.lock() {
                        all.push(prev);
                    }
                }
            }
            let old_path = match d.status() {
                git2::Delta::Added => "/dev/null".to_string(),
                _ => d
                    .old_file()
                    .path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "/dev/null".to_string()),
            };
            let new_path = match d.status() {
                git2::Delta::Deleted => "/dev/null".to_string(),
                _ => d
                    .new_file()
                    .path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "/dev/null".to_string()),
            };
            let header = String::from_utf8_lossy(h.header()).trim_end().to_string();
            if let Ok(mut hk) = cur.lock() {
                *hk = Some(GitDiffHunk {
                    old_path,
                    new_path,
                    old_start: h.old_start(),
                    old_lines: h.old_lines(),
                    new_start: h.new_start(),
                    new_lines: h.new_lines(),
                    header,
                    lines: Vec::new(),
                });
            }
            true
        };
        let mut line_cb =
            |_d: git2::DiffDelta, _h: Option<git2::DiffHunk>, line: git2::DiffLine| -> bool {
                let content_bytes = line.content();
                let content = String::from_utf8_lossy(content_bytes).to_string();
                let origin = line.origin();
                let lead_trimmed = content.trim_start_matches('\n');
                let kind = match origin {
                    '+' => GitDiffLineKind::Addition,
                    '-' => GitDiffLineKind::Deletion,
                    ' ' => GitDiffLineKind::Context,
                    _ => {
                        if lead_trimmed.starts_with("\\ No newline") || origin == '=' {
                            GitDiffLineKind::NoNewline
                        } else {
                            GitDiffLineKind::Context
                        }
                    }
                };
                let (old_line, new_line, trimmed) = match kind {
                    GitDiffLineKind::Addition => (
                        None,
                        line.new_lineno(),
                        content.trim_end_matches('\n').to_string(),
                    ),
                    GitDiffLineKind::Deletion => (
                        line.old_lineno(),
                        None,
                        content.trim_end_matches('\n').to_string(),
                    ),
                    GitDiffLineKind::Context => (
                        line.old_lineno(),
                        line.new_lineno(),
                        content.trim_end_matches('\n').to_string(),
                    ),
                    GitDiffLineKind::NoNewline => {
                        (None, None, lead_trimmed.trim_end_matches('\n').to_string())
                    }
                };
                if let Ok(mut hk) = cur.lock() {
                    if let Some(h) = hk.as_mut() {
                        h.lines.push(GitDiffLine {
                            kind,
                            content: trimmed,
                            old_line,
                            new_line,
                        });
                    }
                }
                true
            };
        diff.foreach(
            &mut file_cb,
            Some(&mut binary_cb),
            Some(&mut hunk_cb),
            Some(&mut line_cb),
        )
        .map_err(map_err)?;
    }
    if let Ok(mut hk) = cur.lock() {
        if let Some(prev) = hk.take() {
            if let Ok(mut all) = hunks.lock() {
                all.push(prev);
            }
        }
    }

    let hunks = hunks.into_inner().unwrap_or_default();

    Ok(GitDiffSummary {
        hunks,
        stat: GitDiffStat {
            files_changed,
            insertions,
            deletions,
        },
    })
}

pub(crate) fn conflict_files_of(repo: &Repository) -> GitResult<Vec<String>> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(false).include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts)).map_err(map_err)?;
    let mut conflict_files = Vec::new();
    for entry in statuses.iter() {
        if entry.status().is_conflicted() {
            if let Some(d) = entry.index_to_workdir() {
                if let Some(p) = d.new_file().path() {
                    conflict_files.push(p.to_string_lossy().into_owned());
                }
            }
        }
    }
    conflict_files.sort();
    Ok(conflict_files)
}

fn remotes_blocking(repo: &Repository) -> GitResult<Vec<GitRemote>> {
    let names = repo.remotes().map_err(map_err)?;
    let mut remotes = Vec::new();
    for name in names.iter().flatten() {
        let url = repo
            .find_remote(name)
            .ok()
            .and_then(|r| r.url().map(|u| u.to_string()))
            .unwrap_or_default();
        let url = sanitize_remote_url(&url);
        let url_type = classify_remote_url(&url);
        remotes.push(GitRemote {
            name: name.to_string(),
            url,
            url_type,
        });
    }
    Ok(remotes)
}

fn in_progress_blocking(repo: &Repository) -> GitResult<Option<GitInProgress>> {
    let Some(operation) = state_to_operation(repo.state()) else {
        return Ok(None);
    };
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(false).include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts)).map_err(map_err)?;
    let mut conflict_files = Vec::new();
    for entry in statuses.iter() {
        if entry.status().is_conflicted() {
            if let Some(d) = entry.index_to_workdir() {
                if let Some(p) = d.new_file().path() {
                    conflict_files.push(p.to_string_lossy().into_owned());
                }
            }
        }
    }
    conflict_files.sort();
    Ok(Some(GitInProgress {
        operation,
        conflict_files,
    }))
}

fn stage_file_blocking(repo: &Repository, path: &str) -> GitResult<()> {
    let mut index = repo.index().map_err(map_err)?;
    index.add_path(Path::new(path)).map_err(|e| {
        if e.code() == git2::ErrorCode::NotFound {
            GitError::not_found(format!("pathspec '{path}' did not match any files"))
        } else {
            map_err(e)
        }
    })?;
    index.write().map_err(map_err)
}

fn unstage_file_blocking(repo: &Repository, path: &str) -> GitResult<()> {
    let head_obj = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    repo.reset_default(head_obj.as_ref().map(|c| c.as_object()), [path])
        .map_err(map_err)?;
    Ok(())
}

pub(crate) fn commit_blocking(repo: &Repository, req: &CommitRequest) -> GitResult<CommitResult> {
    let staged = {
        let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
        let mut index = repo.index().map_err(map_err)?;
        let tree_id = index.write_tree().map_err(map_err)?;
        let index_tree = repo.find_tree(tree_id).map_err(map_err)?;
        let diff = repo
            .diff_tree_to_tree(head_tree.as_ref(), Some(&index_tree), None)
            .map_err(map_err)?;
        diff.deltas().count() > 0
    };
    if !staged && !req.amend {
        return Err(GitError::invalid_params("no staged changes to commit"));
    }

    let sig = repo.signature().map_err(|_| {
        GitError::invalid_params("unable to create commit: author identity unknown")
    })?;

    let mut message = req.message.clone();
    if req.signoff {
        let trailer = format!(
            "Signed-off-by: {} <{}>",
            sig.name().unwrap_or(""),
            sig.email().unwrap_or("")
        );
        if !message.contains(&trailer) {
            if !message.ends_with('\n') {
                message.push('\n');
            }
            message.push('\n');
            message.push_str(&trailer);
            message.push('\n');
        }
    }

    // §6.1: facade-level hook execution (pre-commit / commit-msg / post-commit)
    let (message, hooks) = crate::git2_ops::run_commit_hooks(repo, &message)?;
    let mut message = message.trim_end_matches('\n').to_string();
    if !message.is_empty() {
        message.push('\n');
    }

    let mut index = repo.index().map_err(map_err)?;
    let tree_id = index.write_tree().map_err(map_err)?;
    let tree = repo.find_tree(tree_id).map_err(map_err)?;

    let unsigned = repo
        .config()
        .ok()
        .and_then(|c| c.get_bool("commit.gpgsign").ok())
        .unwrap_or(false);

    let new_commit_oid = if req.amend {
        let head = repo
            .head()
            .map_err(|_| GitError::invalid_params("nothing to amend"))?
            .peel_to_commit()
            .map_err(map_err)?;
        let parents_owned: Vec<git2::Commit> = head.parents().collect();
        let parent_refs: Vec<&git2::Commit> = parents_owned.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &parent_refs)
            .map_err(map_err)?
    } else {
        let head_commit = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let mut parents: Vec<git2::Commit<'_>> = Vec::new();
        if let Some(c) = &head_commit {
            parents.push(c.clone());
        }
        if let Ok(merge_head) = repo.revparse_single("MERGE_HEAD") {
            if let Ok(mc) = merge_head.peel_to_commit() {
                parents.push(mc);
            }
        }
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &parent_refs)
            .map_err(map_err)?
    };

    let new_commit = repo.find_commit(new_commit_oid).map_err(map_err)?;
    let base_tree = if req.amend {
        new_commit.parent(0).ok().and_then(|p| p.tree().ok())
    } else {
        new_commit.parent(0).ok().and_then(|p| p.tree().ok())
    };
    let stats = repo
        .diff_tree_to_tree(base_tree.as_ref(), Some(&tree), None)
        .map_err(map_err)?
        .stats()
        .map_err(map_err)?;

    let (branch, _, _, _) = branch_tracking(repo);

    // §6.1: post-commit hook (best-effort)
    if hooks.hooks_present {
        crate::git2_ops::run_post_commit_hook(repo);
    }

    Ok(CommitResult {
        sha: new_commit_oid.to_string(),
        branch,
        message: new_commit.summary().unwrap_or_default().to_string(),
        files_changed: stats.files_changed() as u32,
        insertions: stats.insertions() as u32,
        deletions: stats.deletions() as u32,
        unsigned: if unsigned { Some(true) } else { None },
        hooks: if hooks.hooks_present {
            Some(hooks)
        } else {
            None
        },
    })
}

fn stash_list_blocking(repo: &mut Repository) -> GitResult<Vec<GitStashEntry>> {
    let mut raw: Vec<(usize, String, git2::Oid)> = Vec::new();
    repo.stash_foreach(|index, message, oid| {
        raw.push((index, message.to_string(), *oid));
        true
    })
    .map_err(map_err)?;
    let mut items = Vec::new();
    for (index, message, oid) in raw {
        let date = repo
            .find_commit(oid)
            .map(|c| iso_date_for_branch(c.time()))
            .unwrap_or_default();
        items.push(GitStashEntry {
            index: index as u32,
            message,
            date,
            branch: String::new(),
        });
    }
    Ok(items)
}

fn stash_count_blocking(repo: &mut Repository) -> GitResult<StashCountResult> {
    let list = stash_list_blocking(repo)?;
    let count = list.len();
    if count == 0 {
        return Ok(StashCountResult {
            count,
            files: Vec::new(),
        });
    }

    let stash_commit = repo
        .revparse_single("stash@{0}")
        .ok()
        .and_then(|o| o.peel_to_commit().ok());
    let base_tree = stash_commit
        .as_ref()
        .and_then(|c| c.parent(0).ok())
        .and_then(|p| p.tree().ok());
    let files = if let Some(sc) = &stash_commit {
        let stash_tree = sc.tree().map_err(map_err)?;
        let diff = repo
            .diff_tree_to_tree(base_tree.as_ref(), Some(&stash_tree), None)
            .map_err(map_err)?;
        per_file_stats(&diff)?
    } else {
        Vec::new()
    };
    Ok(StashCountResult { count, files })
}

fn per_file_stats(diff: &git2::Diff) -> GitResult<Vec<StashCountFile>> {
    per_file_stats_pub(diff)
}

pub(super) fn per_file_stats_pub(diff: &git2::Diff) -> GitResult<Vec<StashCountFile>> {
    let map: std::sync::Mutex<std::collections::BTreeMap<String, (u32, u32)>> =
        std::sync::Mutex::new(std::collections::BTreeMap::new());
    {
        let mut file_cb = |_d: git2::DiffDelta, _f: f32| -> bool { true };
        let mut line_cb =
            |d: git2::DiffDelta, _h: Option<git2::DiffHunk>, line: git2::DiffLine| -> bool {
                let path = d_path(&d, "");
                if path.is_empty() {
                    return true;
                }
                if let Ok(mut m) = map.lock() {
                    let entry = m.entry(path).or_insert((0, 0));
                    match line.origin() {
                        '+' => entry.0 += 1,
                        '-' => entry.1 += 1,
                        _ => {}
                    }
                }
                true
            };
        diff.foreach(&mut file_cb, None, None, Some(&mut line_cb))
            .map_err(map_err)?;
    }
    Ok(map
        .into_inner()
        .unwrap_or_default()
        .into_iter()
        .map(|(path, (insertions, deletions))| StashCountFile {
            path,
            insertions,
            deletions,
        })
        .collect())
}

fn d_path(d: &git2::DiffDelta, fallback: &str) -> String {
    d.new_file()
        .path()
        .or_else(|| d.old_file().path())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| fallback.to_string())
}

/// Apply (or reverse-apply) a unified-diff patch via libgit2.
/// `to_index` selects Index vs Workdir; `reverse` swaps +/- lines because
/// libgit2's apply has no reverse flag.
fn apply_patch(repo: &Repository, patch: &str, to_index: bool, reverse: bool) -> GitResult<()> {
    let effective = if reverse {
        reverse_unified_patch(patch)
    } else {
        patch.to_string()
    };
    let diff = git2::Diff::from_buffer(effective.as_bytes())
        .map_err(|e| GitError::invalid_params(format!("invalid patch: {}", e.message())))?;
    let loc = if to_index {
        git2::ApplyLocation::Index
    } else {
        git2::ApplyLocation::WorkDir
    };
    repo.apply(&diff, loc, None).map_err(|e| {
        let msg = e.message().to_lowercase();
        if msg.contains("hunk") && msg.contains("apply") {
            GitError::conflict(format!("patch does not apply: {}", e.message()))
        } else if msg.contains("already exists") {
            GitError::conflict(e.message().to_string())
        } else {
            map_err(e)
        }
    })
}

/// Swap +/- roles in a unified diff: flip line prefixes, swap hunk ranges and
/// file headers, and drop the "\ No newline" marker to the other side.
fn reverse_unified_patch(patch: &str) -> String {
    let mut out = String::with_capacity(patch.len());
    let mut null_old = false;
    let mut pending_old_path: Option<String> = None;
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("index ") {
            // swap the two blob hashes so added/deleted semantics survive
            // reversal (0000000..abc  ↔  abc..0000000)
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let Some(range) = parts.first().and_then(|p| p.split_once("..")) {
                let swapped = format!("index {}..{}", range.1, range.0);
                if parts.len() >= 2 {
                    out.push_str(&format!("{swapped} {}", parts[1]));
                } else {
                    out.push_str(&swapped);
                }
            } else {
                out.push_str(line);
            }
        } else if let Some(rest) = line.strip_prefix("new file mode ") {
            out.push_str("deleted file mode ");
            out.push_str(rest);
        } else if let Some(rest) = line.strip_prefix("deleted file mode ") {
            out.push_str("new file mode ");
            out.push_str(rest);
        } else if let Some(rest) = line.strip_prefix("--- ") {
            if rest == "/dev/null" {
                // added file: /dev/null moves to the new side on reversal
                null_old = true;
            } else {
                pending_old_path = Some(reverse_side(rest));
                out.push_str(line);
            }
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            if null_old {
                // --- /dev/null, +++ b/X  →  --- a/X, +++ /dev/null
                out.push_str("--- ");
                out.push_str(&reverse_side(rest));
                out.push('\n');
                out.push_str("+++ /dev/null\n");
                null_old = false;
            } else if rest == "/dev/null" {
                // --- a/X, +++ /dev/null  →  --- /dev/null, +++ b/X
                out.push_str("--- /dev/null\n");
                let old_path = pending_old_path.take().unwrap_or_default();
                out.push_str("+++ ");
                out.push_str(&old_path);
                out.push('\n');
            } else {
                out.push_str(line);
            }
        } else if line.starts_with("@@ ") {
            out.push_str(&reverse_hunk_header(line));
        } else if let Some(rest) = line.strip_prefix('+') {
            out.push('-');
            out.push_str(rest);
        } else if let Some(rest) = line.strip_prefix('-') {
            out.push('+');
            out.push_str(rest);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

fn reverse_side(path: &str) -> String {
    match path {
        "/dev/null" => "/dev/null".to_string(),
        _ if path.starts_with("a/") => format!("b/{}", &path[2..]),
        _ if path.starts_with("b/") => format!("a/{}", &path[2..]),
        _ => path.to_string(),
    }
}

fn reverse_hunk_header(line: &str) -> String {
    // @@ -o,ol +n,nl @@ … → @@ -n,nl +o,ol @@  (prefixes stay -old +new)
    let end = match line.find(" @@") {
        Some(i) => i,
        None => return line.to_string(),
    };
    let core = &line[3..end];
    let parts: Vec<&str> = core.split_whitespace().collect();
    if parts.len() < 2 {
        return line.to_string();
    }
    let new_range = parts[1].trim_start_matches('+');
    let old_range = parts[0].trim_start_matches('-');
    format!("@@ -{new_range} +{old_range}{}", &line[end..])
}

/// Stateless git2 backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Git2Backend;

impl Git2Backend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl GitBackend for Git2Backend {
    fn name(&self) -> &'static str {
        "git2"
    }

    async fn status(&self, repo: &Path) -> GitResult<GitStatus> {
        blocking(repo, |repo| status_blocking(&repo, true)).await
    }

    async fn log(&self, repo: &Path, query: &LogQuery) -> GitResult<Vec<GitCommitInfo>> {
        let query = query.clone();
        blocking(repo, move |repo| log_blocking(&repo, &query)).await
    }

    async fn branches(&self, repo: &Path, remote: bool) -> GitResult<Vec<GitBranch>> {
        blocking(repo, move |repo| branches_blocking(&repo, remote)).await
    }

    async fn diff(
        &self,
        repo: &Path,
        staged: bool,
        path: Option<&str>,
        unified: u32,
    ) -> GitResult<GitDiffSummary> {
        let path = path.map(|s| s.to_string());
        blocking(repo, move |repo| {
            diff_blocking(&repo, staged, path.as_deref(), unified)
        })
        .await
    }

    async fn remotes(&self, repo: &Path) -> GitResult<Vec<GitRemote>> {
        blocking(repo, |repo| remotes_blocking(&repo)).await
    }

    async fn in_progress(&self, repo: &Path) -> GitResult<Option<GitInProgress>> {
        blocking(repo, |repo| in_progress_blocking(&repo)).await
    }

    async fn stage_file(&self, repo: &Path, path: &str) -> GitResult<()> {
        let path = path.to_string();
        blocking(repo, move |repo| stage_file_blocking(&repo, &path)).await
    }

    async fn unstage_file(&self, repo: &Path, path: &str) -> GitResult<()> {
        let path = path.to_string();
        blocking(repo, move |repo| unstage_file_blocking(&repo, &path)).await
    }

    async fn commit(&self, repo: &Path, req: CommitRequest) -> GitResult<CommitResult> {
        blocking(repo, move |repo| commit_blocking(&repo, &req)).await
    }

    async fn stash_list(&self, repo: &Path) -> GitResult<Vec<GitStashEntry>> {
        blocking(repo, |mut repo| stash_list_blocking(&mut repo)).await
    }

    async fn stash_count(&self, repo: &Path) -> GitResult<StashCountResult> {
        blocking(repo, |mut repo| stash_count_blocking(&mut repo)).await
    }

    async fn commit_files(
        &self,
        repo: &Path,
        commit: &str,
    ) -> GitResult<Vec<crate::types::CommitFileEntry>> {
        let commit = commit.to_string();
        blocking(repo, move |repo| {
            crate::git2_ops::commit_files_blocking(&repo, &commit)
        })
        .await
    }

    async fn commit_file_diff(
        &self,
        repo: &Path,
        commit: &str,
        path: &str,
        unified: u32,
    ) -> GitResult<GitDiffSummary> {
        let commit = commit.to_string();
        let path = path.to_string();
        blocking(repo, move |repo| {
            crate::git2_ops::commit_file_diff_blocking(&repo, &commit, &path, unified)
        })
        .await
    }

    async fn remote_url(&self, repo: &Path, remote: &str) -> GitResult<String> {
        let remote = remote.to_string();
        blocking(repo, move |repo| {
            crate::git2_ops::remote_url_blocking(&repo, &remote)
        })
        .await
    }

    async fn config_get(&self, repo: &Path, key: &str) -> GitResult<Option<String>> {
        let key = key.to_string();
        blocking(repo, move |repo| match repo.config() {
            Ok(cfg) => match cfg.get_string(&key) {
                Ok(v) => Ok(Some(v)),
                Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
                Err(e) => Err(map_err(e)),
            },
            Err(e) => Err(map_err(e)),
        })
        .await
    }

    async fn config_set(&self, repo: &Path, key: &str, value: &str, global: bool) -> GitResult<()> {
        let key = key.to_string();
        let value = value.to_string();
        blocking(repo, move |repo| {
            let cfg = repo.config().map_err(map_err)?;
            let mut target = if global {
                cfg.open_level(git2::ConfigLevel::Global).map_err(map_err)?
            } else {
                cfg.open_level(git2::ConfigLevel::Local).map_err(map_err)?
            };
            target.set_str(&key, &value).map_err(map_err)
        })
        .await
    }

    async fn checkout_branch(&self, repo: &Path, branch: &str) -> GitResult<String> {
        let branch = branch.to_string();
        blocking(repo, move |repo| {
            let reference = repo
                .find_reference(&format!("refs/heads/{branch}"))
                .map_err(|_| GitError::not_found(format!("branch '{branch}' not found")))?;
            let obj = reference.peel(git2::ObjectType::Commit).map_err(map_err)?;
            repo.checkout_tree(&obj, Some(git2::build::CheckoutBuilder::new().force()))
                .map_err(map_err)?;
            repo.set_head(&format!("refs/heads/{branch}"))
                .map_err(map_err)?;
            Ok(branch)
        })
        .await
    }

    async fn reset_to_commit(&self, repo: &Path, commit: &str, mode: &str) -> GitResult<()> {
        let commit = commit.to_string();
        let mode = mode.to_string();
        blocking(repo, move |repo| {
            let obj = repo
                .revparse_single(&commit)
                .map_err(|_| GitError::not_found(format!("commit '{commit}' not found")))?;
            let ty = match mode.as_str() {
                "soft" => git2::ResetType::Soft,
                "mixed" | "" => git2::ResetType::Mixed,
                "hard" => git2::ResetType::Hard,
                other => {
                    return Err(GitError::invalid_params(format!(
                        "unknown reset mode '{other}'"
                    )))
                }
            };
            repo.reset(&obj, ty, None).map_err(map_err)
        })
        .await
    }

    async fn stage_hunk(&self, repo: &Path, patch: &str) -> GitResult<()> {
        let patch = patch.to_string();
        blocking(repo, move |repo| apply_patch(&repo, &patch, true, false)).await
    }

    async fn unstage_hunk(&self, repo: &Path, patch: &str) -> GitResult<()> {
        let patch = patch.to_string();
        blocking(repo, move |repo| apply_patch(&repo, &patch, true, true)).await
    }

    async fn revert_hunk(&self, repo: &Path, patch: &str) -> GitResult<()> {
        let patch = patch.to_string();
        blocking(repo, move |repo| apply_patch(&repo, &patch, false, true)).await
    }

    async fn stash_push(
        &self,
        repo: &Path,
        opts: crate::types::StashPushOptions,
    ) -> GitResult<bool> {
        blocking(repo, move |mut repo| {
            crate::git2_ops::stash_push_blocking(&mut repo, &opts)
        })
        .await
    }

    async fn stash_pop(&self, repo: &Path, index: usize) -> GitResult<()> {
        blocking(repo, move |mut repo| {
            crate::git2_ops::stash_pop_apply_blocking(&mut repo, index, true)
        })
        .await
    }

    async fn stash_apply(&self, repo: &Path, index: usize) -> GitResult<()> {
        blocking(repo, move |mut repo| {
            crate::git2_ops::stash_pop_apply_blocking(&mut repo, index, false)
        })
        .await
    }

    async fn stash_drop(&self, repo: &Path, index: usize) -> GitResult<()> {
        blocking(repo, move |mut repo| {
            crate::git2_ops::stash_drop_blocking(&mut repo, index)
        })
        .await
    }

    async fn stash_show(
        &self,
        repo: &Path,
        index: usize,
    ) -> GitResult<crate::types::StashCountResult> {
        blocking(repo, move |mut repo| {
            crate::git2_ops::stash_show_blocking(&mut repo, index)
        })
        .await
    }

    async fn merge(
        &self,
        repo: &Path,
        branch: &str,
        opts: crate::types::MergeOptions,
    ) -> GitResult<crate::types::MergeResult> {
        let branch = branch.to_string();
        blocking(repo, move |mut repo| {
            crate::git2_ops::merge_blocking(&mut repo, &branch, &opts)
        })
        .await
    }

    async fn merge_continue(
        &self,
        repo: &Path,
        message: Option<&str>,
    ) -> GitResult<crate::types::MergeResult> {
        let message = message.map(|m| m.to_string());
        blocking(repo, move |mut repo| {
            crate::git2_ops::merge_continue_blocking(&mut repo, message.as_deref())
        })
        .await
    }

    async fn merge_abort(&self, repo: &Path) -> GitResult<()> {
        blocking(repo, |mut repo| {
            crate::git2_ops::merge_abort_blocking(&mut repo)
        })
        .await
    }

    async fn rebase(&self, repo: &Path, onto: &str) -> GitResult<crate::types::RebaseResult> {
        let onto = onto.to_string();
        blocking(repo, move |mut repo| {
            crate::git2_ops::rebase_blocking(&mut repo, &onto)
        })
        .await
    }

    async fn rebase_continue(
        &self,
        repo: &Path,
        message: Option<&str>,
    ) -> GitResult<crate::types::RebaseResult> {
        let message = message.map(|m| m.to_string());
        blocking(repo, move |mut repo| {
            crate::git2_ops::rebase_continue_blocking(&mut repo, message.as_deref())
        })
        .await
    }

    async fn rebase_skip(&self, repo: &Path) -> GitResult<crate::types::RebaseResult> {
        blocking(repo, move |mut repo| {
            crate::git2_ops::rebase_skip_blocking(&mut repo)
        })
        .await
    }

    async fn rebase_abort(&self, repo: &Path) -> GitResult<()> {
        blocking(repo, |mut repo| {
            crate::git2_ops::rebase_abort_blocking(&mut repo)
        })
        .await
    }

    async fn cherry_pick(
        &self,
        repo: &Path,
        commit: &str,
        no_commit: bool,
    ) -> GitResult<crate::types::MergeResult> {
        let commit = commit.to_string();
        blocking(repo, move |mut repo| {
            crate::git2_ops::cherry_pick_blocking(&mut repo, &commit, no_commit)
        })
        .await
    }

    async fn revert_commit(
        &self,
        repo: &Path,
        commit: &str,
    ) -> GitResult<crate::types::MergeResult> {
        let commit = commit.to_string();
        blocking(repo, move |mut repo| {
            crate::git2_ops::revert_commit_blocking(&mut repo, &commit)
        })
        .await
    }

    async fn fetch(
        &self,
        repo: &Path,
        remote: &str,
        branch: Option<&str>,
        prune: bool,
    ) -> GitResult<crate::types::FetchResult> {
        let remote = remote.to_string();
        let branch = branch.map(|b| b.to_string());
        blocking(repo, move |mut repo| {
            crate::git2_ops::fetch_blocking(&mut repo, &remote, branch.as_deref(), prune)
        })
        .await
    }

    async fn push(
        &self,
        repo: &Path,
        remote: &str,
        branch: &str,
        force: bool,
        set_upstream: bool,
    ) -> GitResult<crate::types::PushResult> {
        let remote = remote.to_string();
        let branch = branch.to_string();
        blocking(repo, move |mut repo| {
            crate::git2_ops::push_blocking(&mut repo, &remote, &branch, force, set_upstream)
        })
        .await
    }

    async fn pull(
        &self,
        repo: &Path,
        remote: &str,
        branch: Option<&str>,
    ) -> GitResult<crate::types::PullResult> {
        let remote = remote.to_string();
        let branch = branch.map(|b| b.to_string());
        blocking(repo, move |mut repo| {
            crate::git2_ops::pull_blocking(&mut repo, &remote, branch.as_deref())
        })
        .await
    }

    async fn worktree_list(&self, repo: &Path) -> GitResult<Vec<crate::types::WorktreeInfo>> {
        blocking(repo, |repo| crate::git2_ops::worktree_list_blocking(&repo)).await
    }

    async fn worktree_add(
        &self,
        repo: &Path,
        branch: &str,
        path: &Path,
        create_branch: bool,
    ) -> GitResult<()> {
        let branch = branch.to_string();
        let path = path.to_path_buf();
        blocking(repo, move |repo| {
            crate::git2_ops::worktree_add_blocking(&repo, &branch, &path, create_branch)
        })
        .await
    }

    async fn is_linked_worktree(&self, repo: &Path) -> GitResult<bool> {
        blocking(repo, |repo| {
            crate::git2_ops::is_linked_worktree_blocking(&repo)
        })
        .await
    }

    async fn is_dirty(&self, repo: &Path) -> GitResult<bool> {
        blocking(repo, |repo| crate::git2_ops::is_dirty_blocking(&repo)).await
    }
}
