//! Facade-level smoke: exercise every `facade::*` public function through a
//! single repo journey on each backend. Covers the wrapper/delegation layer
//! (the parity tests drive the backend trait methods directly, so the facade
//! stayed at ~14% line coverage without this file).

use anureo_git::backend::LogQuery;
use anureo_git::types::{CommitRequest, MergeOptions, StashPushOptions};

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

/// First hunk of a `git diff -U3` output as an applyable patch.
fn extract_first_hunk(diff: &str) -> String {
    let mut out = String::new();
    let mut in_hunk = false;
    for line in diff.lines() {
        if line.starts_with("diff --git") {
            out.clear();
            out.push_str(line);
            out.push('\n');
        } else if line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ")
        {
            out.push_str(line);
            out.push('\n');
        } else if line.starts_with("@@ ") {
            if in_hunk {
                break;
            }
            in_hunk = true;
            out.push_str(line);
            out.push('\n');
        } else if in_hunk {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn run_all(backend: &str) {
    std::env::set_var("ANUREO_GIT_BACKEND", backend);
    let r = rt();
    let repo = crate::common::FixtureRepo::new("facadesmoke");
    repo.commit_file("a.txt", "one\ntwo\nthree\n", "first");
    repo.commit_file("b.txt", "bee\n", "second");

    // local bare origin for fetch/push/pull
    let origin = repo.dir.parent().unwrap().join(format!(
        "{}-origin.git",
        repo.dir.file_name().unwrap().to_string_lossy()
    ));
    repo.git_at(&origin, &["init", "--bare", "-b", "main"]);
    repo.git(&["remote", "add", "origin", origin.to_string_lossy().as_ref()]);
    repo.git(&["push", "-u", "origin", "main"]);

    let p = repo.path().to_path_buf();

    // ── read path ────────────────────────────────────────────────────────
    r.block_on(anureo_git::facade::status(&p)).expect("status");
    r.block_on(anureo_git::facade::log(
        &p,
        &LogQuery {
            limit: 5,
            skip: 0,
            branch: None,
            file_path: None,
        },
    ))
    .expect("log");
    r.block_on(anureo_git::facade::branches(&p, true))
        .expect("branches");
    r.block_on(anureo_git::facade::diff(&p, false, None, 3))
        .expect("diff");
    r.block_on(anureo_git::facade::remotes(&p)).expect("remotes");
    r.block_on(anureo_git::facade::in_progress(&p))
        .expect("in_progress");
    r.block_on(anureo_git::facade::commit_files(&p, "HEAD"))
        .expect("commit_files");
    r.block_on(anureo_git::facade::commit_file_diff(&p, "HEAD", "a.txt", 3))
        .expect("commit_file_diff");
    let url = r
        .block_on(anureo_git::facade::remote_url(&p, "origin"))
        .expect("remote_url");
    assert!(url.contains("origin.git"), "remote_url={url}");
    r.block_on(anureo_git::facade::config_get(&p, "user.name"))
        .expect("config_get");
    r.block_on(anureo_git::facade::config_set(&p, "smoke.test", "1", false))
        .expect("config_set");
    r.block_on(anureo_git::facade::checkout_branch(&p, "main"))
        .expect("checkout_branch");
    assert!(!r
        .block_on(anureo_git::facade::is_dirty(&p))
        .expect("is_dirty"));
    assert!(!r
        .block_on(anureo_git::facade::is_linked_worktree(&p))
        .expect("is_linked_worktree"));
    let wts = r
        .block_on(anureo_git::facade::worktree_list(&p))
        .expect("worktree_list");
    assert_eq!(wts.len(), 1, "main worktree only");

    // escape hatches
    r.block_on(anureo_git::facade::run_raw(
        Some(&p),
        &["rev-parse", "--show-toplevel"],
    ))
    .expect("run_raw");

    // ── staging / commit ─────────────────────────────────────────────────
    std::fs::write(repo.dir.join("a.txt"), "one\nTWO\nthree\nfour\n").unwrap();
    r.block_on(anureo_git::facade::diff(&p, false, None, 3))
        .expect("diff dirty");
    r.block_on(anureo_git::facade::stage_file(&p, "a.txt"))
        .expect("stage_file");
    r.block_on(anureo_git::facade::diff(&p, true, None, 3))
        .expect("diff staged");
    let cr = r
        .block_on(anureo_git::facade::commit(
            &p,
            CommitRequest {
                message: "third".into(),
                amend: false,
                signoff: false,
            },
        ))
        .expect("commit");
    assert_eq!(cr.files_changed, 1);

    std::fs::write(repo.dir.join("b.txt"), "bee\nchanged\n").unwrap();
    r.block_on(anureo_git::facade::stage_file(&p, "b.txt"))
        .expect("stage b");
    r.block_on(anureo_git::facade::unstage_file(&p, "b.txt"))
        .expect("unstage_file");

    // ── hunk ops ─────────────────────────────────────────────────────────
    std::fs::write(repo.dir.join("a.txt"), "one\ntwo\nTHREE\nfour\nfive\n").unwrap();
    let diff_text = repo.git(&["diff", "-U3", "--", "a.txt"]);
    let patch = extract_first_hunk(&diff_text);
    assert!(!patch.is_empty(), "patch must extract: {diff_text}");
    r.block_on(anureo_git::facade::stage_hunk(&p, &patch))
        .unwrap_or_else(|e| panic!("stage_hunk failed: {e}\npatch:\n{patch}"));
    r.block_on(anureo_git::facade::unstage_hunk(&p, &patch))
        .expect("unstage_hunk");
    r.block_on(anureo_git::facade::revert_hunk(&p, &patch))
        .expect("revert_hunk");
    r.block_on(anureo_git::facade::run_apply_raw(
        Some(&p),
        &["apply", "--cached"],
        &patch,
    ))
    .expect("run_apply_raw");
    repo.git(&["reset", "--hard", "HEAD"]);
    repo.git(&["clean", "-fd"]);

    // ── stash write domain ───────────────────────────────────────────────
    std::fs::write(repo.dir.join("a.txt"), "one\ntwo\nthree\nfour\nfive\nsix\n").unwrap();
    std::fs::write(repo.dir.join("u.txt"), "untracked\n").unwrap();
    let pushed = r
        .block_on(anureo_git::facade::stash_push(
            &p,
            StashPushOptions {
                message: Some("wip".into()),
                include_untracked: true,
                keep_index: false,
            },
        ))
        .expect("stash_push");
    assert!(pushed);
    let list = r
        .block_on(anureo_git::facade::stash_list(&p))
        .expect("stash_list");
    assert_eq!(list.len(), 1);
    r.block_on(anureo_git::facade::stash_count(&p))
        .expect("stash_count");
    r.block_on(anureo_git::facade::stash_show(&p, 0))
        .expect("stash_show");
    r.block_on(anureo_git::facade::stash_apply(&p, 0))
        .expect("stash_apply");
    r.block_on(anureo_git::facade::stash_drop(&p, 0))
        .expect("stash_drop");
    r.block_on(anureo_git::facade::stash_push(
        &p,
        StashPushOptions::default(),
    ))
    .expect("stash_push 2");
    r.block_on(anureo_git::facade::stash_pop(&p, 0))
        .expect("stash_pop");
    repo.git(&["reset", "--hard", "HEAD"]);
    repo.git(&["clean", "-fd"]);

    // ── reset ────────────────────────────────────────────────────────────
    r.block_on(anureo_git::facade::reset_to_commit(&p, "HEAD", "hard"))
        .expect("reset_to_commit");

    // ── rebase state machine ─────────────────────────────────────────────
    repo.git(&["checkout", "-b", "topic"]);
    std::fs::write(repo.dir.join("a.txt"), "topic one\ntwo\nthree\n").unwrap();
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "topic change"]);
    repo.git(&["checkout", "main"]);
    std::fs::write(repo.dir.join("a.txt"), "main one\ntwo\nthree\n").unwrap();
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "main change"]);

    let res = r
        .block_on(anureo_git::facade::rebase(&p, "topic"))
        .expect("rebase");
    if res.conflicted {
        // skip this conflict
        let s = r
            .block_on(anureo_git::facade::rebase_skip(&p))
            .expect("rebase_skip");
        assert!(s.completed || s.conflicted);
    }

    // another conflict, then abort
    repo.git_raw(&["rebase", "--abort"]);
    std::fs::write(repo.dir.join("a.txt"), "abort one\ntwo\nthree\n").unwrap();
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "abort prep"]);
    let res = r
        .block_on(anureo_git::facade::rebase(&p, "topic"))
        .expect("rebase 2");
    if res.conflicted {
        r.block_on(anureo_git::facade::rebase_abort(&p))
            .expect("rebase_abort");
    }

    // conflict resolved via continue
    repo.git(&["checkout", "main"]);
    repo.git(&["reset", "--hard", "HEAD~1"]);
    repo.git(&["checkout", "-B", "topic2"]);
    std::fs::write(repo.dir.join("a.txt"), "topic2 one\ntwo\nthree\n").unwrap();
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "topic2"]);
    repo.git(&["checkout", "main"]);
    std::fs::write(repo.dir.join("a.txt"), "main2 one\ntwo\nthree\n").unwrap();
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "main2"]);
    let res = r
        .block_on(anureo_git::facade::rebase(&p, "topic2"))
        .expect("rebase 3");
    if res.conflicted {
        std::fs::write(repo.dir.join("a.txt"), "resolved\n").unwrap();
        repo.git(&["add", "."]);
        let c = r
            .block_on(anureo_git::facade::rebase_continue(&p, Some("resolved")))
            .expect("rebase_continue");
        assert!(c.completed || c.conflicted);
    }
    repo.git(&["checkout", "main"]);
    repo.git_raw(&["rebase", "--abort"]);

    // ── merge state machine ──────────────────────────────────────────────
    repo.git(&["checkout", "-B", "side"]);
    std::fs::write(repo.dir.join("a.txt"), "side one\ntwo\nthree\n").unwrap();
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "side"]);
    repo.git(&["checkout", "main"]);
    let m = r
        .block_on(anureo_git::facade::merge(
            &p,
            "side",
            MergeOptions {
                squash: false,
                message: Some("merge side".into()),
                no_ff: false,
            },
        ))
        .expect("merge");
    if m.conflicted {
        let c = r
            .block_on(anureo_git::facade::merge_continue(&p, Some("merged")))
            .expect("merge_continue");
        assert!(c.merge_commit.is_some() || c.conflicted);
        r.block_on(anureo_git::facade::merge_abort(&p))
            .expect("merge_abort");
    }
    repo.git_raw(&["merge", "--abort"]);

    // ff + squash merges
    repo.git(&["checkout", "-B", "side2"]);
    std::fs::write(repo.dir.join("c.txt"), "c\n").unwrap();
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "side2"]);
    repo.git(&["checkout", "main"]);
    let m = r
        .block_on(anureo_git::facade::merge(
            &p,
            "side2",
            MergeOptions {
                squash: false,
                message: None,
                no_ff: false,
            },
        ))
        .expect("merge ff");
    assert!(m.fast_forward || !m.conflicted);
    repo.git(&["reset", "--hard", "HEAD~1"]);
    let m = r
        .block_on(anureo_git::facade::merge(
            &p,
            "side2",
            MergeOptions {
                squash: true,
                message: Some("squashed".into()),
                no_ff: true,
            },
        ))
        .expect("merge squash");
    assert!(m.squashed || m.conflicted);
    repo.git(&["reset", "--hard", "HEAD~1"]);

    // ── cherry-pick / revert ─────────────────────────────────────────────
    let cp = r
        .block_on(anureo_git::facade::cherry_pick(&p, "side2", false))
        .expect("cherry_pick");
    assert!(!cp.conflicted);
    let rv = r
        .block_on(anureo_git::facade::revert_commit(&p, "HEAD"))
        .expect("revert_commit");
    assert!(!rv.conflicted);

    // ── fetch / push / pull ──────────────────────────────────────────────
    let other = repo.dir.parent().unwrap().join(format!(
        "{}-other",
        repo.dir.file_name().unwrap().to_string_lossy()
    ));
    std::fs::create_dir_all(&other).unwrap();
    repo.git_at(&other, &["clone", origin.to_string_lossy().as_ref(), "."]);
    repo.git_at(&other, &["config", "user.name", "T"]);
    repo.git_at(&other, &["config", "user.email", "t@e.com"]);
    std::fs::write(other.join("remote.txt"), "remote\n").unwrap();
    repo.git_at(&other, &["add", "."]);
    repo.git_at(&other, &["commit", "-m", "remote change"]);
    repo.git_at(&other, &["push", "origin", "main"]);

    let f = r
        .block_on(anureo_git::facade::fetch(&p, "origin", Some("main"), true))
        .expect("fetch");
    assert!(!f.fetched_refs.is_empty());
    let pl = r
        .block_on(anureo_git::facade::pull(&p, "origin", Some("main")))
        .expect("pull");
    assert!(pl.fast_forward || pl.merge_commit.is_some() || !pl.conflicts.is_empty());
    let psh = r
        .block_on(anureo_git::facade::push(&p, "origin", "main", false, true))
        .expect("push");
    assert!(!psh.remote_sha.is_empty());

    // ── worktree add ─────────────────────────────────────────────────────
    let wt_path = repo.dir.parent().unwrap().join(format!(
        "{}-wt",
        repo.dir.file_name().unwrap().to_string_lossy()
    ));
    r.block_on(anureo_git::facade::worktree_add(
        &p, "wt-smoke", &wt_path, true,
    ))
    .expect("worktree_add");
    let wts = r
        .block_on(anureo_git::facade::worktree_list(&p))
        .expect("worktree_list 2");
    assert!(wts.len() >= 2, "main + added worktree");
    std::fs::remove_dir_all(&wt_path).ok();
    repo.git(&["worktree", "prune"]);
}

#[test]
fn facade_smoke_full_journey_git2() {
    run_all("git2");
}

#[test]
fn facade_smoke_full_journey_cli() {
    run_all("cli");
}
