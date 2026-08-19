//! git2-native write/state-machine operations: merge, rebase, cherry-pick,
//! revert, stash writes, remote fetch/push with credential callbacks,
//! worktree domain, and hook execution (design doc §6.1/§6.2).

use std::path::{Path, PathBuf};

use git2::{AnnotatedCommit, Repository};

use crate::error::{GitError, GitErrorKind, GitResult};
use crate::types::{
    CommitFileEntry, CommitRequest, FetchResult, HookOutcome, MergeOptions, MergeResult,
    PullResult, PushResult, RebaseResult, StashCountResult, StashPushOptions, WorktreeInfo,
};

use super::git2_backend::{branch_tracking, commit_blocking, conflict_files_of, map_err};

pub(super) fn merge_blocking(
    repo: &mut Repository,
    branch: &str,
    opts: &MergeOptions,
) -> GitResult<MergeResult> {
    let reference = repo
        .find_reference(&format!("refs/heads/{branch}"))
        .or_else(|_| repo.find_reference(&format!("refs/remotes/{branch}")))
        .map_err(|_| GitError::not_found(format!("branch '{branch}' not found")))?;
    let annotated: AnnotatedCommit = repo
        .reference_to_annotated_commit(&reference)
        .map_err(map_err)?;

    let (analysis, _pref) = repo.merge_analysis(&[&annotated]).map_err(map_err)?;

    if analysis.is_up_to_date() {
        return Ok(MergeResult {
            fast_forward: true,
            merge_commit: None,
            conflicts: vec![],
            conflicted: false,
            squashed: false,
        });
    }

    if analysis.is_fast_forward() && !opts.no_ff && !opts.squash {
        let target = repo.find_commit(annotated.id()).map_err(map_err)?;
        let obj = repo
            .find_object(target.id(), Some(git2::ObjectType::Commit))
            .map_err(map_err)?;
        repo.checkout_tree(&obj, None).map_err(map_err)?;
        let refname = format!("refs/heads/{}", branch_tracking(repo).0);
        repo.reference(&refname, target.id(), true, "merge: fast-forward")
            .map_err(map_err)?;
        return Ok(MergeResult {
            fast_forward: true,
            merge_commit: None,
            conflicts: vec![],
            conflicted: false,
            squashed: false,
        });
    }

    // Normal merge (or --no-ff / --squash): apply to index+workdir without commit.
    let mut mopts = git2::MergeOptions::new();
    mopts.fail_on_conflict(false);
    repo.merge(&[&annotated], Some(&mut mopts), None)
        .map_err(map_err)?;

    let conflicts = conflict_files_of(repo)?;
    if !conflicts.is_empty() {
        return Ok(MergeResult {
            fast_forward: false,
            merge_commit: None,
            conflicts,
            conflicted: true,
            squashed: false,
        });
    }

    if opts.squash {
        // Squash semantics: changes staged, MERGE_HEAD dropped, HEAD untouched.
        repo.cleanup_state().map_err(map_err)?;
        return Ok(MergeResult {
            fast_forward: false,
            merge_commit: None,
            conflicts: vec![],
            conflicted: false,
            squashed: true,
        });
    }

    let message = opts
        .message
        .clone()
        .unwrap_or_else(|| format!("Merge branch '{branch}'"));
    let req = CommitRequest {
        message,
        amend: false,
        signoff: false,
    };
    let result = commit_blocking(repo, &req)?;
    Ok(MergeResult {
        fast_forward: false,
        merge_commit: Some(result.sha),
        conflicts: vec![],
        conflicted: false,
        squashed: false,
    })
}

pub(super) fn merge_continue_blocking(
    repo: &mut Repository,
    message: Option<&str>,
) -> GitResult<MergeResult> {
    let conflicts = conflict_files_of(repo)?;
    if !conflicts.is_empty() {
        return Ok(MergeResult {
            conflicts,
            conflicted: true,
            ..Default::default()
        });
    }
    // bug#2 fix: an explicit message must survive the continue path —
    // fall back to .git/MERGE_MSG when the caller did not supply one.
    let msg = message
        .map(|m| m.to_string())
        .or_else(|| std::fs::read_to_string(repo.path().join("MERGE_MSG")).ok())
        .unwrap_or_else(|| "Merge".to_string());
    let result = commit_blocking(
        repo,
        &CommitRequest {
            message: msg,
            amend: false,
            signoff: false,
        },
    )?;
    Ok(MergeResult {
        fast_forward: false,
        merge_commit: Some(result.sha),
        conflicts: vec![],
        conflicted: false,
        squashed: false,
    })
}

pub(super) fn merge_abort_blocking(repo: &mut Repository) -> GitResult<()> {
    let head = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    repo.cleanup_state().map_err(map_err)?;
    if let Some(head) = head {
        let obj = repo
            .find_object(head.id(), Some(git2::ObjectType::Commit))
            .map_err(map_err)?;
        repo.reset(&obj, git2::ResetType::Hard, None)
            .map_err(map_err)?;
    }
    Ok(())
}

fn rebase_step(mut rebase: git2::Rebase<'_>, repo: &Repository) -> GitResult<RebaseResult> {
    let sig = repo
        .signature()
        .map_err(|_| GitError::invalid_params("author identity unknown"))?;
    loop {
        match rebase.next() {
            Some(Ok(_op)) => {
                if rebase.operation_current().is_some() {
                    // index has conflicts for this operation
                    let conflicts = conflict_files_of(repo)?;
                    if !conflicts.is_empty() {
                        return Ok(RebaseResult {
                            conflicts,
                            conflicted: true,
                            completed: false,
                        });
                    }
                }
                rebase.commit(None, &sig, None).map_err(map_err)?;
            }
            None => {
                rebase.finish(Some(&sig)).map_err(map_err)?;
                return Ok(RebaseResult {
                    conflicts: vec![],
                    conflicted: false,
                    completed: true,
                });
            }
            Some(Err(e)) => {
                let conflicts = conflict_files_of(repo)?;
                if !conflicts.is_empty() {
                    return Ok(RebaseResult {
                        conflicts,
                        conflicted: true,
                        completed: false,
                    });
                }
                return Err(map_err(e));
            }
        }
    }
}

pub(super) fn rebase_blocking(repo: &mut Repository, onto: &str) -> GitResult<RebaseResult> {
    let reference = repo
        .find_reference(&format!("refs/heads/{onto}"))
        .or_else(|_| repo.find_reference(&format!("refs/remotes/{onto}")))
        .map_err(|_| GitError::not_found(format!("rebase target '{onto}' not found")))?;
    let upstream = repo
        .reference_to_annotated_commit(&reference)
        .map_err(|e| GitError::not_found(format!("rebase target '{onto}': {}", e.message())))?;

    let mut ropts = git2::RebaseOptions::new();
    let rebase = repo
        .rebase(None, Some(&upstream), None, Some(&mut ropts))
        .map_err(map_err)?;
    rebase_step(rebase, repo)
}

pub(super) fn rebase_continue_blocking(
    repo: &mut Repository,
    _message: Option<&str>,
) -> GitResult<RebaseResult> {
    let conflicts = conflict_files_of(repo)?;
    if !conflicts.is_empty() {
        return Ok(RebaseResult {
            conflicts,
            conflicted: true,
            completed: false,
        });
    }
    let sig = repo
        .signature()
        .map_err(|_| GitError::invalid_params("author identity unknown"))?;
    let mut rebase = repo.open_rebase(None).map_err(map_err)?;
    // commit the resolved operation first (no-op if none staged)
    if rebase.operation_current().is_some() {
        rebase.commit(None, &sig, None).map_err(map_err)?;
    }
    rebase_step(rebase, repo)
}

pub(super) fn rebase_skip_blocking(repo: &mut Repository) -> GitResult<RebaseResult> {
    let sig = repo
        .signature()
        .map_err(|_| GitError::invalid_params("author identity unknown"))?;
    let mut rebase = repo.open_rebase(None).map_err(map_err)?;
    // abort current operation state, then continue with the next
    let mut copts = git2::build::CheckoutBuilder::new();
    copts.force();
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    if let Some(t) = &head_tree {
        repo.checkout_tree(t.as_object(), Some(&mut copts)).ok();
    }
    if rebase.operation_current().is_some() {
        rebase.commit(None, &sig, None).ok();
    }
    rebase_step(rebase, repo)
}

pub(super) fn rebase_abort_blocking(repo: &mut Repository) -> GitResult<()> {
    let mut rebase = repo.open_rebase(None).map_err(map_err)?;
    rebase.abort().map_err(map_err)?;
    Ok(())
}

pub(super) fn cherry_pick_blocking(
    repo: &mut Repository,
    commit_ref: &str,
    no_commit: bool,
) -> GitResult<MergeResult> {
    let commit = repo
        .revparse_single(commit_ref)
        .map_err(|_| GitError::not_found(format!("commit '{commit_ref}' not found")))?
        .peel_to_commit()
        .map_err(map_err)?;
    let mut opts = git2::CherrypickOptions::new();
    repo.cherrypick(&commit, Some(&mut opts)).map_err(map_err)?;
    let conflicts = conflict_files_of(repo)?;
    if !conflicts.is_empty() {
        return Ok(MergeResult {
            conflicts,
            conflicted: true,
            ..Default::default()
        });
    }
    if no_commit {
        return Ok(MergeResult::default());
    }
    let msg = commit.summary().unwrap_or("cherry-pick").to_string();
    let result = commit_blocking(
        repo,
        &CommitRequest {
            message: msg,
            amend: false,
            signoff: false,
        },
    )?;
    Ok(MergeResult {
        merge_commit: Some(result.sha),
        ..Default::default()
    })
}

pub(super) fn revert_commit_blocking(
    repo: &mut Repository,
    commit_ref: &str,
) -> GitResult<MergeResult> {
    let commit = repo
        .revparse_single(commit_ref)
        .map_err(|_| GitError::not_found(format!("commit '{commit_ref}' not found")))?
        .peel_to_commit()
        .map_err(map_err)?;
    let mut opts = git2::RevertOptions::new();
    let _ = &mut opts;
    repo.revert(&commit, Some(&mut opts)).map_err(map_err)?;
    let conflicts = conflict_files_of(repo)?;
    if !conflicts.is_empty() {
        return Ok(MergeResult {
            conflicts,
            conflicted: true,
            ..Default::default()
        });
    }
    // --no-edit semantics: commit with the revert message libgit2 staged.
    let msg = std::fs::read_to_string(repo.path().join("REVERT_MSG"))
        .unwrap_or_else(|_| format!("Revert \"{}\"", commit.summary().unwrap_or("")));
    let msg = msg
        .lines()
        .take_while(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let result = commit_blocking(
        repo,
        &CommitRequest {
            message: msg,
            amend: false,
            signoff: false,
        },
    )?;
    Ok(MergeResult {
        merge_commit: Some(result.sha),
        ..Default::default()
    })
}

pub(super) fn stash_push_blocking(
    repo: &mut Repository,
    opts: &StashPushOptions,
) -> GitResult<bool> {
    // libgit2 stash_save cannot include untracked files — delegate that combo.
    if opts.include_untracked {
        return Err(GitError::unsupported("stash_push(include_untracked)"));
    }
    let sig = repo
        .signature()
        .map_err(|_| GitError::invalid_params("author identity unknown"))?;
    let mut flags = git2::StashFlags::empty();
    if opts.keep_index {
        flags |= git2::StashFlags::KEEP_INDEX;
    }
    let message = opts.message.as_deref().unwrap_or("");
    repo.stash_save(
        &sig,
        message,
        if flags.is_empty() { None } else { Some(flags) },
    )
    .map(|oid| !oid.is_zero())
    .map_err(map_err)
}

pub(super) fn stash_pop_apply_blocking(
    repo: &mut Repository,
    index: usize,
    pop: bool,
) -> GitResult<()> {
    let mut opts = git2::StashApplyOptions::new();
    let res = if pop {
        repo.stash_pop(index, Some(&mut opts))
    } else {
        repo.stash_apply(index, Some(&mut opts))
    };
    res.map_err(|e| {
        let conflicts = conflict_files_of(repo).unwrap_or_default();
        if !conflicts.is_empty() {
            GitError::conflict("stash applied with conflicts")
        } else {
            map_err(e)
        }
    })
}

pub(super) fn stash_drop_blocking(repo: &mut Repository, index: usize) -> GitResult<()> {
    repo.stash_drop(index).map_err(map_err)
}

pub(super) fn stash_show_blocking(
    repo: &mut Repository,
    index: usize,
) -> GitResult<StashCountResult> {
    let stash_ref = repo
        .revparse_single(&format!("stash@{{{index}}}"))
        .map_err(|_| GitError::not_found(format!("stash@{{{index}}} not found")))?
        .peel_to_commit()
        .map_err(map_err)?;
    let base = stash_ref.parent(0).ok().and_then(|p| p.tree().ok());
    let tree = stash_ref.tree().map_err(map_err)?;
    let diff = repo
        .diff_tree_to_tree(base.as_ref(), Some(&tree), None)
        .map_err(map_err)?;
    let files = super::git2_backend::per_file_stats_pub(&diff)?;
    Ok(StashCountResult { count: 1, files })
}

// ── Remote operations with credentials (§6.2) ──────────────────────────

fn ssh_key_candidates() -> Vec<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default();
    let ssh = home.join(".ssh");
    ["id_ed25519", "id_ecdsa", "id_rsa"]
        .iter()
        .map(|k| ssh.join(k))
        .collect()
}

/// Credential callback per §6.2: GITHUB_TOKEN PAT for https,
/// ~/.ssh/id_* keys for ssh URLs.
fn make_remote_callbacks() -> git2::RemoteCallbacks<'static> {
    let mut cbs = git2::RemoteCallbacks::new();
    cbs.credentials(move |url, username_from_url, allowed| {
        if allowed.contains(git2::CredentialType::SSH_KEY) {
            for key in ssh_key_candidates() {
                if key.exists() {
                    return git2::Cred::ssh_key(
                        username_from_url.unwrap_or("git"),
                        None,
                        &key,
                        None,
                    );
                }
            }
        }
        if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            if let Ok(token) = std::env::var("GITHUB_TOKEN") {
                let user = username_from_url.unwrap_or("x-access-token").to_string();
                return git2::Cred::userpass_plaintext(&user, &token);
            }
            if let Ok(token) = std::env::var("GIT_TOKEN") {
                return git2::Cred::userpass_plaintext("oauth2", &token);
            }
        }
        let _ = url;
        Err(git2::Error::from_str("no credentials available"))
    });
    cbs
}

fn is_auth_error(e: &git2::Error) -> bool {
    let msg = e.message().to_lowercase();
    msg.contains("authentication")
        || msg.contains("credential")
        || msg.contains("401")
        || msg.contains("permission denied")
        || msg.contains("unsupported auth")
        || msg.contains("no credentials available")
        || msg.contains("publickey")
}

pub(super) fn fetch_blocking(
    repo: &mut Repository,
    remote_name: &str,
    branch: Option<&str>,
    prune: bool,
) -> GitResult<FetchResult> {
    let before = remote_refs_snapshot(repo, remote_name);
    let mut remote = repo
        .find_remote(remote_name)
        .map_err(|_| GitError::not_found(format!("remote '{remote_name}' not found")))?;
    let refspecs: Vec<String> = branch
        .map(|b| format!("refs/heads/{b}:refs/remotes/{remote_name}/{b}"))
        .into_iter()
        .collect();
    let refspec_refs: Vec<&str> = refspecs.iter().map(|s| s.as_str()).collect();

    let cbs = make_remote_callbacks();
    let mut fo = git2::FetchOptions::new();
    fo.remote_callbacks(cbs);
    if prune {
        fo.prune(git2::FetchPrune::On);
    }
    let res = remote.fetch(&refspec_refs, Some(&mut fo), None);
    if let Err(e) = res {
        if is_auth_error(&e) {
            // §6.2 fallback: re-run via git CLI (facade::fetch handles it)
            return Err(GitError::new(
                GitErrorKind::Auth,
                format!("git2 fetch auth failed: {}", e.message()),
            ));
        }
        return Err(map_err(e));
    }
    let after = remote_refs_snapshot(repo, remote_name);
    let mut refs = Vec::new();
    for (name, new) in &after {
        let old = before.get(name).cloned().unwrap_or_default();
        if old != *new {
            refs.push(crate::types::FetchedRef {
                ref_name: name.clone(),
                old_sha: old,
                new_sha: new.clone(),
            });
        }
    }
    Ok(FetchResult {
        fetched_refs: refs,
        fetch_via: None,
    })
}

fn remote_refs_snapshot(
    repo: &Repository,
    remote_name: &str,
) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    let prefix = format!("refs/remotes/{remote_name}/");
    if let Ok(refs) = repo.references() {
        for r in refs.flatten() {
            if let (Some(name), Some(oid)) = (r.name(), r.target()) {
                if name.starts_with(&prefix) {
                    map.insert(
                        name.trim_start_matches("refs/remotes/").to_string(),
                        oid.to_string(),
                    );
                }
            }
        }
    }
    map
}

pub(super) fn push_blocking(
    repo: &mut Repository,
    remote_name: &str,
    branch: &str,
    force: bool,
    set_upstream: bool,
) -> GitResult<PushResult> {
    let local_ref = format!("refs/heads/{branch}");
    let remote_ref = format!("refs/heads/{branch}");
    let refspec = if force {
        format!("+{local_ref}:{remote_ref}")
    } else {
        format!("{local_ref}:{remote_ref}")
    };

    let mut remote = repo
        .find_remote(remote_name)
        .map_err(|_| GitError::not_found(format!("remote '{remote_name}' not found")))?;
    let push_failed: std::sync::Arc<std::sync::Mutex<Option<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let failed_slot = push_failed.clone();
    let mut cbs = make_remote_callbacks();
    cbs.push_update_reference(
        move |_refname: &str, status: Option<&str>| -> Result<(), git2::Error> {
            if let Some(s) = status {
                if let Ok(mut slot) = failed_slot.lock() {
                    *slot = Some(s.to_string());
                }
            }
            Ok(())
        },
    );
    let mut po = git2::PushOptions::new();
    po.remote_callbacks(cbs);
    let res = remote.push(&[&refspec], Some(&mut po));
    if let Err(e) = res {
        if is_auth_error(&e) {
            return Err(GitError::new(
                GitErrorKind::Auth,
                format!("git2 push auth failed: {}", e.message()),
            ));
        }
        return Err(map_err(e));
    }
    if let Some(f) = push_failed.lock().ok().and_then(|s| s.clone()) {
        return Err(GitError::conflict(format!("push rejected: {f}")));
    }
    if set_upstream {
        let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch(branch, &head_commit, false)
            .ok()
            .and_then(|mut b| {
                b.set_upstream(Some(&format!("{remote_name}/{branch}")))
                    .ok()
            });
    }
    let remote_sha = repo
        .revparse_single(&format!("{remote_name}/{branch}"))
        .ok()
        .map(|o| o.id().to_string())
        .unwrap_or_default();
    Ok(PushResult {
        remote_sha,
        push_via: None,
    })
}

pub(super) fn pull_blocking(
    repo: &mut Repository,
    remote_name: &str,
    branch: Option<&str>,
) -> GitResult<PullResult> {
    fetch_blocking(repo, remote_name, branch, false)?;
    let tracking = branch_tracking(repo);
    let upstream = tracking
        .1
        .or_else(|| branch.map(|b| format!("{remote_name}/{b}")))
        .ok_or_else(|| GitError::invalid_params("no upstream configured"))?;
    let use_rebase = repo
        .config()
        .ok()
        .and_then(|c| c.get_bool("pull.rebase").ok())
        .unwrap_or(false);
    if use_rebase {
        let rb = rebase_blocking(repo, &upstream)?;
        Ok(PullResult {
            fast_forward: false,
            merge_commit: None,
            conflicts: rb.conflicts,
        })
    } else {
        let merge_res = merge_blocking(repo, &upstream, &MergeOptions::default())?;
        Ok(PullResult {
            fast_forward: merge_res.fast_forward,
            merge_commit: merge_res.merge_commit,
            conflicts: merge_res.conflicts,
        })
    }
}

// ── Commit files / worktree domain ──────────────────────────────────────

/// libgit2 escapes non-UTF-8 path bytes as `\nnn` octal sequences; decode
/// them back so paths match what the git CLI emits (UTF-8 lossless).
fn decode_escaped_path(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let mut bytes = Vec::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let mut num = 0u32;
            let mut n = 0usize;
            let mut digits = Vec::new();
            while n < 3 {
                if let Some(&d) = chars.peek() {
                    if let Some(v) = d.to_digit(8) {
                        num = num * 8 + v;
                        digits.push(d);
                        chars.next();
                        n += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            if n == 3 {
                bytes.push(num as u8);
            } else {
                bytes.push(b'\\');
                for d in digits {
                    bytes.push(d as u8);
                }
            }
        } else {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn file_path(d: &git2::DiffDelta) -> String {
    d.new_file()
        .path()
        .or_else(|| d.old_file().path())
        .map(|p| {
            let s = p.to_string_lossy();
            let s = if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                &s[1..s.len() - 1]
            } else {
                &s
            };
            decode_escaped_path(s)
        })
        .unwrap_or_default()
}

pub(super) fn commit_files_blocking(
    repo: &Repository,
    commit_ref: &str,
) -> GitResult<Vec<CommitFileEntry>> {
    let commit = repo
        .revparse_single(commit_ref)
        .map_err(|_| GitError::not_found(format!("commit '{commit_ref}' not found")))?
        .peel_to_commit()
        .map_err(map_err)?;
    let tree = commit.tree().map_err(map_err)?;
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let diff = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)
        .map_err(map_err)?;

    let stats: std::sync::Mutex<std::collections::BTreeMap<String, (u32, u32)>> =
        std::sync::Mutex::new(std::collections::BTreeMap::new());
    let kinds: std::sync::Mutex<std::collections::BTreeMap<String, String>> =
        std::sync::Mutex::new(std::collections::BTreeMap::new());
    {
        let mut file_cb = |d: git2::DiffDelta, _f: f32| -> bool {
            let path = file_path(&d);
            if path.is_empty() {
                return true;
            }
            if let Ok(mut m) = stats.lock() {
                m.entry(path.clone()).or_insert((0, 0));
            }
            if let Ok(mut k) = kinds.lock() {
                k.insert(
                    path,
                    match d.status() {
                        git2::Delta::Added => "added".to_string(),
                        git2::Delta::Deleted => "deleted".to_string(),
                        git2::Delta::Renamed => "renamed".to_string(),
                        git2::Delta::Copied => "copied".to_string(),
                        _ => "modified".to_string(),
                    },
                );
            }
            true
        };
        let mut line_cb =
            |d: git2::DiffDelta, _h: Option<git2::DiffHunk>, line: git2::DiffLine| -> bool {
                let path = file_path(&d);
                if path.is_empty() {
                    return true;
                }
                if let Ok(mut m) = stats.lock() {
                    if let Some(e) = m.get_mut(&path) {
                        match line.origin() {
                            '+' => e.0 += 1,
                            '-' => e.1 += 1,
                            _ => {}
                        }
                    }
                }
                true
            };
        diff.foreach(&mut file_cb, None, None, Some(&mut line_cb))
            .map_err(map_err)?;
    }
    let stats = stats.into_inner().unwrap_or_default();
    let kinds = kinds.into_inner().unwrap_or_default();
    Ok(stats
        .into_iter()
        .map(|(path, (ins, del))| CommitFileEntry {
            status: kinds.get(&path).cloned().unwrap_or_default(),
            insertions: Some(ins),
            deletions: Some(del),
            path,
        })
        .collect())
}

pub(super) fn commit_file_diff_blocking(
    repo: &Repository,
    commit_ref: &str,
    path: &str,
    unified: u32,
) -> GitResult<crate::types::GitDiffSummary> {
    let commit = repo
        .revparse_single(commit_ref)
        .map_err(|_| GitError::not_found(format!("commit '{commit_ref}' not found")))?
        .peel_to_commit()
        .map_err(map_err)?;
    let tree = commit.tree().map_err(map_err)?;
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let mut opts = git2::DiffOptions::new();
    opts.context_lines(unified).pathspec(path);
    let diff = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut opts))
        .map_err(map_err)?;
    super::git2_backend::collect_diff_summary_pub(&diff)
}

pub(super) fn remote_url_blocking(repo: &Repository, remote: &str) -> GitResult<String> {
    let url = repo
        .find_remote(remote)
        .map_err(|_| GitError::not_found(format!("remote '{remote}' not found")))?
        .url()
        .map(|u| u.to_string())
        .unwrap_or_default();
    if url.is_empty() {
        return Err(GitError::not_found(format!("remote '{remote}' not found")));
    }
    Ok(url)
}

pub(super) fn worktree_list_blocking(repo: &Repository) -> GitResult<Vec<WorktreeInfo>> {
    let mut out = Vec::new();
    // Main worktree: `git worktree list` includes it; libgit2's worktrees()
    // only enumerates linked ones — synthesize the main entry for parity.
    if let Some(workdir) = repo.workdir() {
        let (branch, head) = if repo.head_detached().unwrap_or(false) {
            (
                None,
                repo.head()
                    .ok()
                    .and_then(|h| h.target().map(|t| t.to_string())),
            )
        } else {
            (
                repo.head()
                    .ok()
                    .and_then(|h| h.shorthand().map(|s| s.to_string())),
                repo.head()
                    .ok()
                    .and_then(|h| h.target().map(|t| t.to_string())),
            )
        };
        let name = workdir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push(WorktreeInfo {
            name,
            path: workdir.to_string_lossy().into_owned(),
            branch,
            head,
        });
    }
    let names = repo.worktrees().map_err(map_err)?;
    for name in names.iter().flatten() {
        if let Ok(wt) = repo.find_worktree(name) {
            let path = wt.path().to_string_lossy().into_owned();
            let (branch, head) = Repository::open(wt.path())
                .map(|r| {
                    let b = if r.head_detached().unwrap_or(false) {
                        None
                    } else {
                        r.head()
                            .ok()
                            .and_then(|h| h.shorthand().map(|s| s.to_string()))
                    };
                    let h = r
                        .head()
                        .ok()
                        .and_then(|h| h.target().map(|t| t.to_string()));
                    (b, h)
                })
                .unwrap_or((None, None));
            out.push(WorktreeInfo {
                name: name.to_string(),
                path,
                branch,
                head,
            });
        }
    }
    Ok(out)
}

pub(super) fn worktree_add_blocking(
    repo: &Repository,
    branch: &str,
    path: &Path,
    create_branch: bool,
) -> GitResult<()> {
    let head_oid = repo
        .head()
        .map_err(|_| GitError::invalid_params("repository has no commits"))?
        .target()
        .ok_or_else(|| GitError::internal("HEAD has no target"))?;
    let reference = if create_branch {
        Some(
            repo.reference(
                &format!("refs/heads/{branch}"),
                head_oid,
                false,
                "worktree add",
            )
            .map_err(|e| {
                if e.code() == git2::ErrorCode::Exists {
                    GitError::conflict(format!("branch '{branch}' already exists"))
                } else {
                    map_err(e)
                }
            })?,
        )
    } else {
        None
    };
    let opts = git2::WorktreeAddOptions::new();
    let mut opts = opts;
    if let Some(r) = &reference {
        opts.reference(Some(r));
    }
    repo.worktree(branch, path, Some(&opts)).map_err(map_err)?;
    Ok(())
}

pub(super) fn is_linked_worktree_blocking(repo: &Repository) -> GitResult<bool> {
    let common = repo.commondir();
    let own = repo.path();
    Ok(common != own)
}

pub(super) fn is_dirty_blocking(repo: &Repository) -> GitResult<bool> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    let statuses = repo.statuses(Some(&mut opts)).map_err(map_err)?;
    Ok(!statuses.is_empty())
}

// ── Hooks (§6.1) ───────────────────────────────────────────────────────

fn find_sh() -> Option<PathBuf> {
    // Git for Windows ships a real sh.exe — preferred over the system32
    // bash.exe WSL shim which fails silently when WSL is not installed.
    if let Ok(pf) = std::env::var("ProgramFiles") {
        for cand in [
            format!("{pf}\\Git\\bin\\sh.exe"),
            format!("{pf}\\Git\\usr\\bin\\sh.exe"),
        ] {
            let p = PathBuf::from(&cand);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(';').chain(path_var.split(':')) {
            if dir.is_empty() {
                continue;
            }
            let lower = dir.to_ascii_lowercase();
            if lower.contains("system32") || lower.contains("syswow64") {
                continue;
            }
            for exe in ["sh.exe", "bash.exe", "sh", "bash"] {
                let p = Path::new(dir).join(exe);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    let p = PathBuf::from("/bin/sh");
    if p.is_file() {
        return Some(p);
    }
    None
}

fn hook_is_executable(hook: &Path) -> bool {
    if !hook.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(hook)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        std::fs::metadata(hook)
            .map(|m| m.len() > 0)
            .unwrap_or(false)
    }
}

fn run_hook(
    repo: &Repository,
    hook_name: &str,
    args: &[&str],
    message: Option<&str>,
) -> std::result::Result<Option<String>, String> {
    let hooks_dir = repo.path().join("hooks");
    let hook = hooks_dir.join(hook_name);
    if !hook_is_executable(&hook) {
        return Ok(None);
    }
    let Some(sh) = find_sh() else {
        return Err("__NO_SH__".to_string());
    };

    let editmsg = repo.path().join("COMMIT_EDITMSG");
    if let Some(m) = message {
        std::fs::write(&editmsg, m).map_err(|e| format!("write COMMIT_EDITMSG: {e}"))?;
    }

    let git_dir = repo
        .path()
        .canonicalize()
        .unwrap_or_else(|_| repo.path().to_path_buf());
    let output = std::process::Command::new(&sh)
        .arg(&hook)
        .args(args)
        .current_dir(repo.workdir().unwrap_or_else(|| Path::new(".")))
        .env("GIT_DIR", &git_dir)
        .env("GIT_EDITOR", ":")
        .output()
        .map_err(|e| format!("spawn hook {hook_name}: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(stderr);
    }
    if hook_name == "commit-msg" {
        if let Ok(rewritten) = std::fs::read_to_string(&editmsg) {
            return Ok(Some(rewritten));
        }
    }
    Ok(None)
}

pub(super) fn run_commit_hooks(
    repo: &Repository,
    message: &str,
) -> GitResult<(String, HookOutcome)> {
    let mut outcome = HookOutcome::default();
    let hooks_present = ["pre-commit", "commit-msg", "post-commit"]
        .iter()
        .any(|h| hook_is_executable(&repo.path().join("hooks").join(h)));
    if !hooks_present {
        return Ok((message.to_string(), outcome));
    }
    outcome.hooks_present = true;

    if let Err(err) = run_hook(repo, "pre-commit", &[], None) {
        if err == "__NO_SH__" {
            outcome.hooks_skipped_no_sh = true;
            tracing::warn!(
                hook = "pre-commit",
                "sh interpreter not found; hooks skipped"
            );
            return Ok((message.to_string(), outcome));
        }
        outcome.failure = Some(err.clone());
        return Err(GitError::conflict(format!("pre-commit hook failed: {err}")));
    }
    outcome.hooks_executed = true;

    let editmsg_path = repo.path().join("COMMIT_EDITMSG");
    let msg_arg = editmsg_path.to_string_lossy().into_owned();
    match run_hook(repo, "commit-msg", &[&msg_arg], Some(message)) {
        Ok(rewritten) => {
            outcome.hooks_executed = true;
            Ok((rewritten.unwrap_or_else(|| message.to_string()), outcome))
        }
        Err(err) if err == "__NO_SH__" => {
            outcome.hooks_skipped_no_sh = true;
            tracing::warn!(
                hook = "commit-msg",
                "sh interpreter not found; hooks skipped"
            );
            Ok((message.to_string(), outcome))
        }
        Err(err) => {
            outcome.failure = Some(err.clone());
            Err(GitError::conflict(format!("commit-msg hook failed: {err}")))
        }
    }
}

pub(super) fn run_post_commit_hook(repo: &Repository) {
    if let Err(e) = run_hook(repo, "post-commit", &[], None) {
        if e != "__NO_SH__" {
            tracing::warn!(hook = "post-commit", error = %e, "post-commit hook failed");
        }
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn decode_escaped_utf8() {
        assert_eq!(decode_escaped_path("a/b.txt"), "a/b.txt");
        assert_eq!(
            decode_escaped_path("\\344\\270\\255/\\346\\226\\207.txt"),
            "中/文.txt"
        );
        assert_eq!(
            decode_escaped_path("\\346\\226\\207\\344\\273\\266.txt"),
            "文件.txt"
        );
    }

    #[test]
    #[serial]
    fn file_path_unquotes() {
        let s = "\"\\344\\270\\255\\346\\226\\207.txt\"";
        let stripped = &s[1..s.len() - 1];
        assert_eq!(decode_escaped_path(stripped), "中文.txt");
    }

    fn init_temp_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let out = std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .expect("git init in test");
        assert!(out.status.success());
        dir
    }

    #[test]
    #[serial]
    fn ssh_key_candidates_from_home() {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("USERPROFILE", home.path());
        std::env::remove_var("HOME");
        let cands = ssh_key_candidates();
        assert_eq!(cands.len(), 3);
        assert!(cands[0].starts_with(home.path().join(".ssh")));
        assert!(cands[0].ends_with("id_ed25519"));
        std::env::remove_var("USERPROFILE");
    }

    #[test]
    #[serial]
    fn is_auth_error_matches_keywords() {
        for msg in [
            "authentication failed",
            "credential callback returned error",
            "request sent but not received (401)",
            "permission denied (publickey)",
            "unsupported auth method",
            "no credentials available",
            "Permission denied (publickey,password)",
        ] {
            let e = git2::Error::from_str(msg);
            assert!(is_auth_error(&e), "must detect auth error: {msg}");
        }
        let plain = git2::Error::from_str("object not found - no match for id");
        assert!(!is_auth_error(&plain));
    }

    #[test]
    #[serial]
    fn find_sh_prefers_git_dir_and_skips_system32() {
        let old_pf = std::env::var("ProgramFiles").ok();
        let old_path = std::env::var("PATH").ok();

        // fake Git-for-Windows layout wins over PATH
        let fake = tempfile::tempdir().unwrap();
        let sh_path = fake.path().join("Git").join("bin").join("sh.exe");
        std::fs::create_dir_all(sh_path.parent().unwrap()).unwrap();
        std::fs::write(&sh_path, "").unwrap();
        std::env::set_var("ProgramFiles", fake.path());
        let found = find_sh();
        assert_eq!(found, Some(sh_path.clone()));
        std::env::remove_var("ProgramFiles");

        // a real sh in PATH beats the system32 bash shim
        let bin = tempfile::tempdir().unwrap();
        let real_sh = bin.path().join("sh.exe");
        std::fs::write(&real_sh, "").unwrap();
        let path_with_shim_and_real =
            format!("C:\\Windows\\System32;{}", bin.path().to_string_lossy());
        std::env::set_var("PATH", path_with_shim_and_real);
        let found = find_sh();
        assert_eq!(found, Some(real_sh), "system32 bash shim must be skipped");

        if let Some(p) = old_pf {
            std::env::set_var("ProgramFiles", p);
        } else {
            std::env::remove_var("ProgramFiles");
        }
        if let Some(p) = old_path {
            std::env::set_var("PATH", p);
        } else {
            std::env::remove_var("PATH");
        }
    }

    #[test]
    #[serial]
    fn hook_is_executable_requires_nonempty() {
        let dir = tempfile::tempdir().unwrap();
        let hook = dir.path().join("pre-commit");
        assert!(!hook_is_executable(&hook), "missing hook");
        std::fs::write(&hook, "").unwrap();
        assert!(!hook_is_executable(&hook), "empty file is not executable");
        std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
        assert!(hook_is_executable(&hook), "non-empty hook is executable");
    }

    #[test]
    #[serial]
    fn run_commit_hooks_no_hooks_is_noop() {
        let dir = init_temp_repo();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let (msg, outcome) = run_commit_hooks(&repo, "some message").unwrap();
        assert_eq!(msg, "some message");
        assert!(!outcome.hooks_present);
        assert!(!outcome.hooks_executed);
    }

    #[test]
    #[serial]
    fn run_commit_hooks_pre_commit_blocks() {
        let dir = init_temp_repo();
        std::fs::create_dir_all(dir.path().join(".git/hooks")).unwrap();
        std::fs::write(
            dir.path().join(".git/hooks/pre-commit"),
            "#!/bin/sh\necho blocked >&2\nexit 1\n",
        )
        .unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let err = run_commit_hooks(&repo, "m").unwrap_err();
        assert!(matches!(err.kind(), GitErrorKind::Conflict));
        assert!(err.message().contains("pre-commit hook failed"));
    }

    #[test]
    #[serial]
    fn run_commit_hooks_commit_msg_rewrites() {
        let dir = init_temp_repo();
        std::fs::create_dir_all(dir.path().join(".git/hooks")).unwrap();
        std::fs::write(
            dir.path().join(".git/hooks/pre-commit"),
            "#!/bin/sh\nexit 0\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".git/hooks/commit-msg"),
            "#!/bin/sh\nprintf 'rewritten message' > \"$1\"\n",
        )
        .unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let (msg, outcome) = run_commit_hooks(&repo, "original").unwrap();
        assert_eq!(msg, "rewritten message");
        assert!(outcome.hooks_present);
        assert!(outcome.hooks_executed);
        assert!(!outcome.hooks_skipped_no_sh);
    }

    #[test]
    #[serial]
    fn run_commit_hooks_skipped_without_sh() {
        let dir = init_temp_repo();
        std::fs::create_dir_all(dir.path().join(".git/hooks")).unwrap();
        std::fs::write(
            dir.path().join(".git/hooks/pre-commit"),
            "#!/bin/sh\nexit 0\n",
        )
        .unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        // make sh undiscoverable, then restore
        let old_pf = std::env::var("ProgramFiles").ok();
        let old_path = std::env::var("PATH").ok();
        std::env::remove_var("ProgramFiles");
        std::env::remove_var("PATH");
        let (msg, outcome) = run_commit_hooks(&repo, "m").unwrap();
        assert_eq!(msg, "m");
        assert!(outcome.hooks_present);
        assert!(
            outcome.hooks_skipped_no_sh,
            "must degrade when sh is missing"
        );
        if let Some(p) = old_pf {
            std::env::set_var("ProgramFiles", p);
        }
        if let Some(p) = old_path {
            std::env::set_var("PATH", p);
        }
    }

    #[test]
    #[serial]
    fn run_post_commit_hook_swallows_failures() {
        let dir = init_temp_repo();
        std::fs::create_dir_all(dir.path().join(".git/hooks")).unwrap();
        std::fs::write(
            dir.path().join(".git/hooks/post-commit"),
            "#!/bin/sh\nfalse\n",
        )
        .unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        run_post_commit_hook(&repo);
        // no panic; failing post-commit is non-fatal
    }
}
