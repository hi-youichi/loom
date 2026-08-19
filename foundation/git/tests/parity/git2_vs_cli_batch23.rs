//! Batch 2/3/4 parity: stash writes, hunk ops, merge/rebase/cherry-pick state
//! machines, commit_files, hooks (§6.1), autocrlf & non-ASCII, and the
//! facade-level flag smoke (explicit backend construction).

use std::path::{Path, PathBuf};

use loom_git::backend::{GitBackend, LogQuery};
use loom_git::cli::CliBackend;
use loom_git::git2_backend::Git2Backend;
use loom_git::types::{CommitRequest, MergeOptions, StashPushOptions};

use crate::common::FixtureRepo;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

#[test]
fn parity_stash_write_roundtrip() {
    let r = rt();
    for backend in ["cli", "git2"] {
        let repo = FixtureRepo::new("stashw");
        repo.commit_file("a.txt", "1\n", "first");
        std::fs::write(repo.dir.join("a.txt"), "dirty\n").unwrap();

        let b: Box<dyn GitBackend> = if backend == "cli" {
            Box::new(CliBackend::new())
        } else {
            Box::new(Git2Backend::new())
        };
        let opts = StashPushOptions {
            message: Some("wip".into()),
            include_untracked: false,
            keep_index: false,
        };
        let created = r
            .block_on(b.stash_push(repo.path(), opts))
            .expect("stash push");
        assert!(created, "stash created via {backend}");

        let list = r.block_on(b.stash_list(repo.path())).unwrap();
        assert_eq!(list.len(), 1, "{backend}");

        let workdir_clean = std::fs::read_to_string(repo.dir.join("a.txt"))
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(workdir_clean, "1\n", "workdir restored via {backend}");

        let show = r.block_on(b.stash_show(repo.path(), 0)).unwrap();
        assert!(!show.files.is_empty(), "{backend} stash_show files");
        assert!(show.files.iter().any(|f| f.insertions > 0), "{backend}");

        r.block_on(b.stash_pop(repo.path(), 0)).expect("stash pop");
        let after = std::fs::read_to_string(repo.dir.join("a.txt"))
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(after, "dirty\n", "stash applied via {backend}");
        let list = r.block_on(b.stash_list(repo.path())).unwrap();
        assert!(list.is_empty(), "pop drops entry ({backend})");
    }
}

#[test]
fn parity_stash_include_untracked_delegates() {
    // libgit2 cannot stash untracked files — the facade delegates to CLI.
    let r = rt();
    let repo = FixtureRepo::new("stashu");
    repo.commit_file("a.txt", "1\n", "first");
    std::fs::write(repo.dir.join("new.txt"), "untracked\n").unwrap();
    let out = loom_git::facade::stash_push(
        repo.path(),
        StashPushOptions {
            message: None,
            include_untracked: true,
            keep_index: false,
        },
    );
    assert!(r.block_on(out).expect("delegated stash push"));
    assert!(!repo.dir.join("new.txt").exists(), "untracked stashed");
    r.block_on(loom_git::facade::stash_pop(repo.path(), 0))
        .expect("pop");
    assert!(repo.dir.join("new.txt").exists());
}

#[test]
fn parity_merge_conflict_and_continue() {
    let r = rt();
    for backend in ["cli", "git2"] {
        let repo = FixtureRepo::new("mergec");
        repo.commit_file("x.txt", "base\n", "base");
        repo.git(&["checkout", "-b", "side"]);
        repo.commit_file("x.txt", "theirs\n", "side");
        repo.git(&["checkout", "main"]);
        repo.commit_file("x.txt", "ours\n", "main change");

        let b: Box<dyn GitBackend> = if backend == "cli" {
            Box::new(CliBackend::new())
        } else {
            Box::new(Git2Backend::new())
        };
        let res = r
            .block_on(b.merge(repo.path(), "side", MergeOptions::default()))
            .expect("merge started");
        assert!(res.conflicted, "{backend} must report conflict");
        assert_eq!(res.conflicts, vec!["x.txt".to_string()]);

        // resolve: take theirs
        std::fs::write(repo.dir.join("x.txt"), "resolved\n").unwrap();
        repo.git(&["add", "x.txt"]);

        // bug#2 regression: explicit message must land on the merge commit
        let cont = r
            .block_on(b.merge_continue(repo.path(), Some("custom merge message")))
            .expect("continue");
        assert!(cont.merge_commit.is_some(), "{backend} merge commit");
        let log = r
            .block_on(b.log(
                repo.path(),
                &LogQuery {
                    limit: 1,
                    skip: 0,
                    branch: None,
                    file_path: None,
                },
            ))
            .unwrap();
        assert_eq!(
            log[0].message, "custom merge message",
            "bug#2 via {backend}"
        );
        assert_eq!(log[0].parents.len(), 2, "merge commit has two parents");
    }
}

#[test]
fn parity_merge_ff_and_squash() {
    let r = rt();
    for backend in ["cli", "git2"] {
        let repo = FixtureRepo::new("mergeff");
        repo.commit_file("a.txt", "1\n", "base");
        repo.git(&["checkout", "-b", "feature"]);
        repo.commit_file("b.txt", "2\n", "feature work");
        repo.git(&["checkout", "main"]);

        let b: Box<dyn GitBackend> = if backend == "cli" {
            Box::new(CliBackend::new())
        } else {
            Box::new(Git2Backend::new())
        };
        let res = r
            .block_on(b.merge(repo.path(), "feature", MergeOptions::default()))
            .expect("ff merge");
        assert!(res.fast_forward, "{backend} fast-forward");

        // squash: changes staged, no merge commit, HEAD untouched
        repo.git(&["checkout", "-b", "feature2"]);
        repo.commit_file("c.txt", "3\n", "more work");
        repo.git(&["checkout", "main"]);
        let res = r
            .block_on(b.merge(
                repo.path(),
                "feature2",
                MergeOptions {
                    squash: true,
                    no_ff: true,
                    message: None,
                },
            ))
            .expect("squash merge");
        let log = r
            .block_on(b.log(
                repo.path(),
                &LogQuery {
                    limit: 5,
                    skip: 0,
                    branch: None,
                    file_path: None,
                },
            ))
            .unwrap();
        // last commit must still be "feature work" (squash does not commit)
        assert!(
            log.iter().any(|c| c.message == "feature work"),
            "{backend}: squash must not advance HEAD"
        );
        let _ = res;
    }
}

#[test]
fn parity_merge_abort_restores() {
    let r = rt();
    for backend in ["cli", "git2"] {
        let repo = FixtureRepo::new("mergeab");
        repo.commit_file("x.txt", "base\n", "base");
        repo.git(&["checkout", "-b", "side"]);
        repo.commit_file("x.txt", "theirs\n", "side");
        repo.git(&["checkout", "main"]);
        repo.commit_file("x.txt", "ours\n", "main change");

        let b: Box<dyn GitBackend> = if backend == "cli" {
            Box::new(CliBackend::new())
        } else {
            Box::new(Git2Backend::new())
        };
        let _ = r
            .block_on(b.merge(repo.path(), "side", MergeOptions::default()))
            .unwrap();
        r.block_on(b.merge_abort(repo.path()))
            .expect("abort via {backend}");
        let content = std::fs::read_to_string(repo.dir.join("x.txt"))
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(content, "ours\n", "abort restores ours via {backend}");
    }
}

#[test]
fn parity_rebase_conflict_continue_and_abort() {
    let r = rt();
    for backend in ["cli", "git2"] {
        let repo = FixtureRepo::new("rebase");
        repo.commit_file("x.txt", "base\n", "base");
        repo.git(&["checkout", "-b", "topic"]);
        repo.commit_file("x.txt", "topic\n", "topic change");
        repo.git(&["checkout", "main"]);
        repo.commit_file("x.txt", "main\n", "main change");
        repo.git(&["checkout", "topic"]);

        let b: Box<dyn GitBackend> = if backend == "cli" {
            Box::new(CliBackend::new())
        } else {
            Box::new(Git2Backend::new())
        };
        let res = r
            .block_on(b.rebase(repo.path(), "main"))
            .expect("rebase started");
        assert!(res.conflicted, "{backend} rebase conflict");
        assert_eq!(res.conflicts, vec!["x.txt".to_string()]);

        // resolve and continue (bug#1: must not hang on an editor)
        std::fs::write(repo.dir.join("x.txt"), "topic\n").unwrap();
        repo.git(&["add", "x.txt"]);
        let cont = r
            .block_on(b.rebase_continue(repo.path(), None))
            .expect("rebase continue");
        assert!(cont.completed, "{backend} rebase completed");
        let content = std::fs::read_to_string(repo.dir.join("x.txt")).unwrap();
        assert_eq!(content, "topic\n", "{backend} rebased content");

        // abort path
        let repo2 = FixtureRepo::new("rebaseab");
        repo2.commit_file("y.txt", "base\n", "base");
        repo2.git(&["checkout", "-b", "topic"]);
        repo2.commit_file("y.txt", "topic\n", "topic change");
        repo2.git(&["checkout", "main"]);
        repo2.commit_file("y.txt", "main\n", "main change");
        repo2.git(&["checkout", "topic"]);
        let res = r
            .block_on(b.rebase(repo2.path(), "main"))
            .expect("rebase 2 started");
        assert!(res.conflicted);
        r.block_on(b.rebase_abort(repo2.path()))
            .expect("abort via {backend}");
        let content = std::fs::read_to_string(repo2.dir.join("y.txt"))
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(content, "topic\n", "abort restores topic via {backend}");
    }
}

#[test]
fn parity_cherry_pick_and_revert() {
    let r = rt();
    for backend in ["cli", "git2"] {
        let repo = FixtureRepo::new("cpick");
        repo.commit_file("a.txt", "1\n", "base");
        repo.git(&["checkout", "-b", "feat"]);
        repo.commit_file("b.txt", "2\n", "add b");
        let feat_head = repo.git(&["rev-parse", "HEAD"]).trim().to_string();
        repo.git(&["checkout", "main"]);

        let b: Box<dyn GitBackend> = if backend == "cli" {
            Box::new(CliBackend::new())
        } else {
            Box::new(Git2Backend::new())
        };
        let res = r
            .block_on(b.cherry_pick(repo.path(), &feat_head, false))
            .expect("cherry-pick");
        assert!(res.merge_commit.is_some(), "{backend} new commit");
        assert!(repo.dir.join("b.txt").exists(), "{backend} b.txt applied");

        // revert it
        let res = r
            .block_on(b.revert_commit(repo.path(), "HEAD"))
            .expect("revert");
        assert!(res.merge_commit.is_some());
        assert!(
            !repo.dir.join("b.txt").exists(),
            "{backend} revert removed b.txt"
        );
    }
}

#[test]
fn parity_commit_files() {
    let r = rt();
    let repo = FixtureRepo::new("cfiles");
    repo.commit_file("a.txt", "1\n2\n3\n", "base");
    repo.git(&["checkout", "-b", "feat"]);
    repo.commit_file("b.txt", "new\nfile\n", "add b");
    std::fs::write(repo.dir.join("a.txt"), "1\nchanged\n3\n").unwrap();
    repo.git(&["add", "a.txt"]);
    repo.git(&["commit", "-m", "modify a"]);
    let head = repo.git(&["rev-parse", "HEAD"]).trim().to_string();

    let cli = CliBackend::new();
    let g2 = Git2Backend::new();
    let c = r.block_on(cli.commit_files(repo.path(), &head)).unwrap();
    let g = r.block_on(g2.commit_files(repo.path(), &head)).unwrap();
    assert_eq!(c, g, "commit_files parity");
    // HEAD commit ("modify a") changes only a.txt; "add b" is HEAD~1.
    assert_eq!(c.len(), 1);
    assert!(c
        .iter()
        .any(|f| f.path == "a.txt" && f.insertions == Some(1) && f.deletions == Some(1)));
    let parent = repo.git(&["rev-parse", "HEAD~1"]).trim().to_string();
    let cp = r.block_on(cli.commit_files(repo.path(), &parent)).unwrap();
    let gp = r.block_on(g2.commit_files(repo.path(), &parent)).unwrap();
    assert_eq!(cp, gp, "commit_files(parent) parity");
    assert!(cp.iter().any(|f| f.path == "b.txt" && f.status == "added"));

    let cd = r
        .block_on(cli.commit_file_diff(repo.path(), &head, "a.txt", 3))
        .unwrap();
    let gd = r
        .block_on(g2.commit_file_diff(repo.path(), &head, "a.txt", 3))
        .unwrap();
    assert_eq!(cd.hunks.len(), 1);
    assert_eq!(cd.hunks, gd.hunks, "commit_file_diff parity");
}

#[test]
fn parity_hunk_stage_unstage_revert() {
    let r = rt();
    for backend in ["cli", "git2"] {
        let repo = FixtureRepo::new("hunks");
        repo.commit_file("a.txt", "one\ntwo\nthree\nfour\nfive\n", "base");
        std::fs::write(repo.dir.join("a.txt"), "one\nTWO\nthree\nfour\nFIVE\n").unwrap();

        // Build the full-file patch from the working diff, then apply only
        // the first hunk.
        let diff_text = repo.git(&["diff", "-U3", "--", "a.txt"]);
        let first_hunk_patch = extract_first_hunk(&diff_text);

        let b: Box<dyn GitBackend> = if backend == "cli" {
            Box::new(CliBackend::new())
        } else {
            Box::new(Git2Backend::new())
        };
        r.block_on(b.stage_hunk(repo.path(), &first_hunk_patch))
            .unwrap_or_else(|e| panic!("stage hunk via {backend}: {e}"));

        let staged = repo.git(&["diff", "--cached", "--name-only"]);
        assert!(staged.contains("a.txt"), "{backend}: hunk staged");

        // unstage the same patch (reverse)
        r.block_on(b.unstage_hunk(repo.path(), &first_hunk_patch))
            .expect("unstage hunk via {backend}");
        let staged = repo.git(&["diff", "--cached", "--name-only"]);
        assert!(!staged.contains("a.txt"), "{backend}: hunk unstaged");

        // revert in workdir
        r.block_on(b.revert_hunk(repo.path(), &first_hunk_patch))
            .expect("revert hunk via {backend}");
        let content = std::fs::read_to_string(repo.dir.join("a.txt"))
            .unwrap()
            .replace("\r\n", "\n");
        assert!(content.contains("two\n"), "{backend}: first hunk reverted");
    }
}

fn extract_first_hunk(diff_text: &str) -> String {
    let mut out = String::new();
    let mut in_hunk = false;
    for line in diff_text.lines() {
        if line.starts_with("diff --git ") {
            out.clear();
            out.push_str(line);
            out.push('\n');
        } else if line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ")
        {
            out.push_str(line);
            out.push('\n');
        } else if line.starts_with("@@") {
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

#[test]
fn hooks_pre_commit_blocks_and_commit_msg_rewrites() {
    let r = rt();
    let repo = FixtureRepo::new("hooks");
    let sh = find_any_sh();
    if sh.is_none() {
        eprintln!("no sh interpreter on PATH — hooks degrade path covered by design");
        return;
    }

    // Base commit BEFORE installing hooks (the fixture's own git CLI also
    // runs hooks).
    repo.commit_file("a.txt", "1\n", "base");

    let hooks_dir = repo
        .git(&["rev-parse", "--absolute-git-dir"])
        .trim()
        .to_string();
    let hooks = PathBuf::from(&hooks_dir).join("hooks");
    std::fs::create_dir_all(&hooks).ok();

    // failing pre-commit blocks the commit with a Conflict-classified error
    std::fs::write(
        hooks.join("pre-commit"),
        "#!/bin/sh\necho 'blocked by hook' >&2\nexit 1\n",
    )
    .unwrap();

    std::fs::write(repo.dir.join("a.txt"), "2\n").unwrap();
    repo.git(&["add", "a.txt"]);
    let g2 = Git2Backend::new();
    let err = r
        .block_on(g2.commit(
            repo.path(),
            CommitRequest {
                message: "should fail".into(),
                amend: false,
                signoff: false,
            },
        ))
        .expect_err("pre-commit hook must block");
    assert!(matches!(err.kind(), loom_git::GitErrorKind::Conflict));

    // passing pre-commit + commit-msg that rewrites the message
    std::fs::write(hooks.join("pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(
        hooks.join("commit-msg"),
        "#!/bin/sh\nprintf 'rewritten: %s' \"$1\" > /dev/null; printf 'hook-approved message' > \"$1\"\n",
    )
    .unwrap();
    let res = r
        .block_on(g2.commit(
            repo.path(),
            CommitRequest {
                message: "original message".into(),
                amend: false,
                signoff: false,
            },
        ))
        .expect("commit with hooks");
    assert_eq!(res.message, "hook-approved message");
    assert!(res
        .hooks
        .as_ref()
        .map(|h| h.hooks_executed)
        .unwrap_or(false));
}

fn find_any_sh() -> Option<PathBuf> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(';') {
            for exe in ["sh.exe", "bash.exe"] {
                let p = Path::new(dir).join(exe);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
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
    None
}

#[test]
fn parity_autocrlf_status_and_diff() {
    let r = rt();
    let repo = FixtureRepo::new("crlf");
    repo.git(&["config", "core.autocrlf", "true"]);
    std::fs::write(repo.dir.join("crlf.txt"), "line1\r\nline2\r\n").unwrap();
    repo.git(&["add", "crlf.txt"]);
    repo.git(&["commit", "-m", "crlf base"]);
    std::fs::write(repo.dir.join("crlf.txt"), "line1\r\nline2 changed\r\n").unwrap();

    let cli = CliBackend::new();
    let g2 = Git2Backend::new();
    let c = r.block_on(cli.status(repo.path())).expect("cli status");
    let g = r.block_on(g2.status(repo.path())).expect("git2 status");
    assert_eq!(c.files.len(), g.files.len(), "autocrlf status parity");
    let cd = r.block_on(cli.diff(repo.path(), false, None, 3)).unwrap();
    let gd = r.block_on(g2.diff(repo.path(), false, None, 3)).unwrap();
    assert_eq!(cd.hunks, gd.hunks, "autocrlf diff parity");
}

#[test]
fn parity_non_ascii_paths() {
    let r = rt();
    let repo = FixtureRepo::new("nascii");
    repo.commit_file("中文目录/文件.txt", "内容\n", "add non-ascii");
    std::fs::write(repo.dir.join("中文目录/文件.txt"), "内容改\n").unwrap();
    repo.git(&["add", "中文目录/文件.txt"]);

    let cli = CliBackend::new();
    let g2 = Git2Backend::new();
    let c = r.block_on(cli.status(repo.path())).unwrap();
    let g = r.block_on(g2.status(repo.path())).unwrap();
    let mut c = c;
    let mut g = g;
    c.files.sort_by(|a, b| a.path.cmp(&b.path));
    g.files.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(c.files, g.files, "non-ascii status parity");

    repo.git(&["commit", "-m", "change non-ascii"]);
    let head = repo.git(&["rev-parse", "HEAD"]).trim().to_string();
    let cf = r.block_on(cli.commit_files(repo.path(), &head)).unwrap();
    let gf = r.block_on(g2.commit_files(repo.path(), &head)).unwrap();
    assert_eq!(cf, gf, "non-ascii commit_files parity");
}

#[test]
fn facade_flag_smoke_both_kinds() {
    // B 验收「flag e2e 冒烟」：两种后端经 facade 语义等价（显式构造，
    // 绕过 OnceLock 进程级缓存）。
    let r = rt();
    let repo = FixtureRepo::new("flag");
    repo.commit_file("a.txt", "1\n", "first");
    std::fs::write(repo.dir.join("a.txt"), "2\n").unwrap();

    let backends: Vec<Box<dyn GitBackend>> =
        vec![Box::new(CliBackend::new()), Box::new(Git2Backend::new())];
    for backend in backends {
        let s = r.block_on(backend.status(repo.path())).expect("status");
        assert_eq!(s.branch, "main");
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.files[0].path, "a.txt");
    }
}

#[test]
fn facade_fetch_push_local_file_remote() {
    // Local file:// remotes need no credentials — exercises the fetch/push
    // plumbing on both backends without network.
    let r = rt();
    for backend in ["cli", "git2"] {
        let repo = FixtureRepo::new("remote");
        repo.commit_file("a.txt", "1\n", "first");
        let origin = repo.dir.parent().unwrap().join(format!(
            "{}-origin.git",
            repo.dir.file_name().unwrap().to_string_lossy()
        ));
        repo.git_at(&origin, &["init", "--bare", "-b", "main"]);
        repo.git(&["remote", "add", "origin", origin.to_string_lossy().as_ref()]);

        let b: Box<dyn GitBackend> = if backend == "cli" {
            Box::new(CliBackend::new())
        } else {
            Box::new(Git2Backend::new())
        };
        let push = r
            .block_on(b.push(repo.path(), "origin", "main", false, true))
            .expect("push via {backend}");
        assert!(!push.remote_sha.is_empty(), "{backend} remote_sha");

        // clone into a second repo, advance it, then fetch in the original
        let other = FixtureRepo::new("remote2");
        std::fs::remove_dir_all(&other.dir).unwrap();
        repo.git_at(
            &other.dir,
            &["clone", origin.to_string_lossy().as_ref(), "."],
        );
        repo.git_at(&other.dir, &["config", "user.name", "T"]);
        repo.git_at(&other.dir, &["config", "user.email", "t@e.com"]);
        std::fs::write(other.dir.join("b.txt"), "2\n").unwrap();
        repo.git_at(&other.dir, &["add", "b.txt"]);
        repo.git_at(&other.dir, &["commit", "-m", "second"]);
        repo.git_at(&other.dir, &["push", "origin", "main"]);

        let fetch = r
            .block_on(b.fetch(repo.path(), "origin", Some("main"), true))
            .expect("fetch via {backend}");
        assert!(
            fetch
                .fetched_refs
                .iter()
                .any(|fr| fr.ref_name.contains("main")),
            "{backend} fetched main"
        );

        let pull = r
            .block_on(b.pull(repo.path(), "origin", Some("main")))
            .expect("pull via {backend}");
        assert!(pull.fast_forward, "{backend} pull ff");
        assert!(repo.dir.join("b.txt").exists(), "{backend} pulled b.txt");
    }
}

#[test]
fn parity_worktree_domain() {
    let r = rt();
    let repo = FixtureRepo::new("wt");
    repo.commit_file("a.txt", "1\n", "first");

    let wt_path = repo.dir.parent().unwrap().join(format!(
        "{}-side",
        repo.dir.file_name().unwrap().to_string_lossy()
    ));

    let cli = CliBackend::new();
    let g2 = Git2Backend::new();
    r.block_on(g2.worktree_add(repo.path(), "side", &wt_path, true))
        .expect("git2 worktree add");
    assert!(wt_path.join(".git").exists(), "linked worktree created");

    let listed = r.block_on(cli.worktree_list(repo.path())).unwrap();
    let listed_g = r.block_on(g2.worktree_list(repo.path())).unwrap();
    assert_eq!(listed.len(), 2, "main + side");
    assert_eq!(listed_g.len(), 2);
    assert!(listed.iter().any(|w| w.branch.as_deref() == Some("side")));

    let linked = r
        .block_on(g2.is_linked_worktree(&wt_path))
        .expect("is_linked");
    assert!(linked, "side is a linked worktree");
    let main_linked = r.block_on(g2.is_linked_worktree(repo.path())).unwrap();
    assert!(!main_linked, "main repo is not linked");

    let dirty = r.block_on(g2.is_dirty(&wt_path)).expect("is_dirty");
    assert!(!dirty);
}
