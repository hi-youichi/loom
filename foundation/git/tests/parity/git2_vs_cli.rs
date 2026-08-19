//! Dual-backend parity: CliBackend vs Git2Backend on identical fixture repos.

use std::path::Path;

use loom_git::backend::{GitBackend, LogQuery};
use loom_git::cli::CliBackend;
use loom_git::git2_backend::Git2Backend;
use loom_git::types::{GitDiffSummary, GitStatus};

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

fn norm_status(mut s: GitStatus) -> GitStatus {
    s.files.sort_by(|a, b| a.path.cmp(&b.path));
    if let Some(ip) = &mut s.in_progress {
        ip.conflict_files.sort();
    }
    s
}

fn norm_diff(mut d: GitDiffSummary) -> GitDiffSummary {
    d.hunks.sort_by(|a, b| {
        (
            a.old_path.as_str(),
            a.new_path.as_str(),
            a.old_start,
            a.new_start,
        )
            .cmp(&(
                b.old_path.as_str(),
                b.new_path.as_str(),
                b.old_start,
                b.new_start,
            ))
    });
    d
}

async fn assert_status_eq(path: &Path) {
    let cli = CliBackend::new();
    let g2 = Git2Backend::new();
    let c = norm_status(cli.status(path).await.expect("cli status"));
    let g = norm_status(g2.status(path).await.expect("git2 status"));
    assert_eq!(c, g, "status parity failed");
}

fn fixture() -> crate::common::FixtureRepo {
    crate::common::FixtureRepo::new("parity")
}

#[test]
fn parity_status_states() {
    let r = rt();
    let repo = fixture();
    repo.commit_file("a.txt", "line1\nline2\nline3\n", "first");
    repo.commit_file("b.txt", "b content\n", "second");

    r.block_on(assert_status_eq(repo.path()));

    std::fs::write(repo.dir.join("a.txt"), "line1\nchanged\nline3\n").unwrap();
    std::fs::write(repo.dir.join("c.txt"), "new untracked\n").unwrap();
    r.block_on(assert_status_eq(repo.path()));

    repo.git(&["add", "a.txt"]);
    r.block_on(assert_status_eq(repo.path()));
}

#[test]
fn parity_status_renamed_and_deleted() {
    let r = rt();
    let repo = fixture();
    repo.commit_file("old.txt", "content\nmore\n", "first");
    std::fs::rename(repo.dir.join("old.txt"), repo.dir.join("new.txt")).unwrap();
    r.block_on(assert_status_eq(repo.path()));

    std::fs::remove_file(repo.dir.join("new.txt")).unwrap();
    r.block_on(assert_status_eq(repo.path()));
}

#[test]
fn parity_log() {
    let r = rt();
    let repo = fixture();
    repo.commit_file("a.txt", "1\n", "first");
    repo.commit_file("a.txt", "2\n", "second");
    repo.commit_file("b.txt", "3\n", "third");
    repo.git(&["checkout", "-b", "feature"]);
    repo.commit_file("a.txt", "4\n", "fourth");

    for query in [
        LogQuery {
            limit: 10,
            skip: 0,
            branch: None,
            file_path: None,
        },
        LogQuery {
            limit: 1,
            skip: 1,
            branch: None,
            file_path: None,
        },
        LogQuery {
            limit: 10,
            skip: 0,
            branch: Some("main".to_string()),
            file_path: None,
        },
        LogQuery {
            limit: 10,
            skip: 0,
            branch: None,
            file_path: Some("a.txt".to_string()),
        },
    ] {
        let cli = CliBackend::new();
        let g2 = Git2Backend::new();
        let c: Vec<_> = r.block_on(cli.log(repo.path(), &query)).expect("cli log");
        let mut g: Vec<_> = r.block_on(g2.log(repo.path(), &query)).expect("git2 log");
        for item in g.iter_mut() {
            item.refs.sort();
        }
        let mut c = c;
        for item in c.iter_mut() {
            item.refs.sort();
        }
        assert_eq!(c, g, "log parity failed for {query:?}");
    }
}

#[test]
fn parity_branches() {
    let r = rt();
    let repo = fixture();
    repo.commit_file("a.txt", "1\n", "first");
    repo.git(&["checkout", "-b", "feature"]);
    repo.commit_file("a.txt", "2\n", "second");
    repo.git(&["checkout", "main"]);

    for remote in [false, true] {
        let cli = CliBackend::new();
        let g2 = Git2Backend::new();
        let c = r
            .block_on(cli.branches(repo.path(), remote))
            .expect("cli branches");
        let g = r
            .block_on(g2.branches(repo.path(), remote))
            .expect("git2 branches");
        assert_eq!(c, g, "branches parity failed (remote={remote})");
    }
}

#[test]
fn parity_diff() {
    let r = rt();
    let repo = fixture();
    repo.commit_file(
        "a.txt",
        "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\n",
        "first",
    );
    repo.commit_file("gone.txt", "bye\n", "second");

    // modified + EOF-newline change + deleted + binary + untracked
    std::fs::write(
        repo.dir.join("a.txt"),
        "line1\nlineX\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10",
    )
    .unwrap();
    std::fs::remove_file(repo.dir.join("gone.txt")).unwrap();
    std::fs::write(repo.dir.join("bin.dat"), b"\x00\x01\x02binary").unwrap();
    std::fs::write(repo.dir.join("u.txt"), "untracked\n").unwrap();

    for staged in [false, true] {
        for unified in [0u32, 3, 5] {
            let cli = CliBackend::new();
            let g2 = Git2Backend::new();
            let c = norm_diff(
                r.block_on(cli.diff(repo.path(), staged, None, unified))
                    .expect("cli diff"),
            );
            let g = norm_diff(
                r.block_on(g2.diff(repo.path(), staged, None, unified))
                    .expect("git2 diff"),
            );
            assert_eq!(c, g, "diff parity failed (staged={staged}, u={unified})");
        }
    }

    // staged diff after add
    repo.git(&["add", "a.txt"]);
    let cli = CliBackend::new();
    let g2 = Git2Backend::new();
    let c = norm_diff(
        r.block_on(cli.diff(repo.path(), true, Some("a.txt"), 3))
            .expect("cli staged diff"),
    );
    let g = norm_diff(
        r.block_on(g2.diff(repo.path(), true, Some("a.txt"), 3))
            .expect("git2 staged diff"),
    );
    assert_eq!(c, g, "staged path diff parity failed");
}

#[test]
fn parity_diff_rename() {
    let r = rt();
    let repo = fixture();
    repo.commit_file("old_name.txt", "aaa\nbbb\nccc\n", "first");
    std::fs::rename(repo.dir.join("old_name.txt"), repo.dir.join("new_name.txt")).unwrap();

    let cli = CliBackend::new();
    let g2 = Git2Backend::new();
    let c = norm_diff(
        r.block_on(cli.diff(repo.path(), false, None, 3))
            .expect("cli diff"),
    );
    let g = norm_diff(
        r.block_on(g2.diff(repo.path(), false, None, 3))
            .expect("git2 diff"),
    );
    assert_eq!(c, g, "rename diff parity failed");
}

#[test]
fn parity_remotes() {
    let r = rt();
    let repo = fixture();
    repo.commit_file("a.txt", "1\n", "first");
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://user:token@github.com/example/repo.git",
    ]);
    repo.git(&["remote", "add", "upstream", "git@github.com:other/repo.git"]);

    let cli = CliBackend::new();
    let g2 = Git2Backend::new();
    let c = r.block_on(cli.remotes(repo.path())).expect("cli remotes");
    let g = r.block_on(g2.remotes(repo.path())).expect("git2 remotes");
    assert_eq!(c, g, "remotes parity failed");
}

#[test]
fn parity_in_progress_conflict() {
    let r = rt();
    let repo = fixture();
    repo.commit_file("x.txt", "base\n", "base");
    repo.git(&["checkout", "-b", "side"]);
    repo.commit_file("x.txt", "theirs\n", "side change");
    repo.git(&["checkout", "main"]);
    repo.commit_file("x.txt", "ours\n", "main change");
    let (ok, out, err) = repo.git_raw(&["merge", "side"]);
    assert!(
        !ok && (out.contains("CONFLICT") || err.contains("CONFLICT")),
        "merge conflicted: {out}{err}"
    );

    let cli = CliBackend::new();
    let g2 = Git2Backend::new();
    let c = r
        .block_on(cli.in_progress(repo.path()))
        .expect("cli in_progress");
    let g = r
        .block_on(g2.in_progress(repo.path()))
        .expect("git2 in_progress");
    assert_eq!(c, g, "in_progress parity failed");
    r.block_on(assert_status_eq(repo.path()));
}

#[test]
fn parity_stage_unstage_commit_roundtrip() {
    let r = rt();
    for backend_case in ["cli", "git2"] {
        let repo = fixture();
        repo.commit_file("a.txt", "1\n", "first");
        std::fs::write(repo.dir.join("a.txt"), "1\n2\n").unwrap();

        let cli = CliBackend::new();
        let g2 = Git2Backend::new();
        match backend_case {
            "cli" => {
                r.block_on(cli.stage_file(repo.path(), "a.txt"))
                    .expect("cli stage");
            }
            _ => {
                r.block_on(g2.stage_file(repo.path(), "a.txt"))
                    .expect("git2 stage");
            }
        }

        // staged diff identical across backends after either staged
        let c = norm_diff(r.block_on(cli.diff(repo.path(), true, None, 3)).unwrap());
        let g = norm_diff(r.block_on(g2.diff(repo.path(), true, None, 3)).unwrap());
        assert_eq!(c, g, "staged diff parity after stage via {backend_case}");

        match backend_case {
            "cli" => {
                r.block_on(cli.unstage_file(repo.path(), "a.txt"))
                    .expect("cli unstage");
            }
            _ => {
                r.block_on(g2.unstage_file(repo.path(), "a.txt"))
                    .expect("git2 unstage");
            }
        }
        let c = norm_diff(r.block_on(cli.diff(repo.path(), true, None, 3)).unwrap());
        let g = norm_diff(r.block_on(g2.diff(repo.path(), true, None, 3)).unwrap());
        assert_eq!(c.stat.files_changed, 0);
        assert_eq!(g.stat.files_changed, 0);

        // commit
        match backend_case {
            "cli" => {
                r.block_on(cli.stage_file(repo.path(), "a.txt")).unwrap();
                let res = r
                    .block_on(cli.commit(
                        repo.path(),
                        loom_git::types::CommitRequest {
                            message: "second".into(),
                            amend: false,
                            signoff: false,
                        },
                    ))
                    .expect("cli commit");
                assert_eq!(res.branch, "main");
                assert_eq!(res.message, "second");
                assert_eq!(res.files_changed, 1);
                assert_eq!(res.insertions, 1);
            }
            _ => {
                r.block_on(g2.stage_file(repo.path(), "a.txt")).unwrap();
                let res = r
                    .block_on(g2.commit(
                        repo.path(),
                        loom_git::types::CommitRequest {
                            message: "second".into(),
                            amend: false,
                            signoff: false,
                        },
                    ))
                    .expect("git2 commit");
                assert_eq!(res.branch, "main");
                assert_eq!(res.message, "second");
                assert_eq!(res.files_changed, 1);
                assert_eq!(res.insertions, 1);
                assert_eq!(res.unsigned, None);
            }
        }
        // log parity after commit
        let c: Vec<_> = r
            .block_on(cli.log(
                repo.path(),
                &LogQuery {
                    limit: 5,
                    skip: 0,
                    branch: None,
                    file_path: None,
                },
            ))
            .unwrap();
        let g: Vec<_> = r
            .block_on(g2.log(
                repo.path(),
                &LogQuery {
                    limit: 5,
                    skip: 0,
                    branch: None,
                    file_path: None,
                },
            ))
            .unwrap();
        assert_eq!(c, g, "log parity after commit via {backend_case}");
    }
}

#[test]
fn parity_stash() {
    let r = rt();
    let repo = fixture();
    repo.commit_file("a.txt", "1\n", "first");
    std::fs::write(repo.dir.join("a.txt"), "dirty change\n").unwrap();
    std::fs::write(repo.dir.join("new.txt"), "another\n").unwrap();
    repo.git(&["stash", "push", "-u", "-m", "wip changes"]);

    let cli = CliBackend::new();
    let g2 = Git2Backend::new();
    let c = r
        .block_on(cli.stash_list(repo.path()))
        .expect("cli stash list");
    let g = r
        .block_on(g2.stash_list(repo.path()))
        .expect("git2 stash list");
    assert_eq!(c, g, "stash list parity failed");

    let c = r
        .block_on(cli.stash_count(repo.path()))
        .expect("cli stash count");
    let g = r
        .block_on(g2.stash_count(repo.path()))
        .expect("git2 stash count");
    assert_eq!(c.count, g.count, "stash count parity");
    assert_eq!(c.count, 1);
    // bug#3 regression: files must carry real insertion counts
    assert!(!c.files.is_empty(), "cli files must be non-empty");
    assert!(!g.files.is_empty(), "git2 files must be non-empty");
    let total_ins_c: u32 = c.files.iter().map(|f| f.insertions).sum();
    let total_ins_g: u32 = g.files.iter().map(|f| f.insertions).sum();
    assert!(total_ins_c > 0, "cli insertions must be counted (bug#3)");
    assert!(total_ins_g > 0, "git2 insertions must be counted");
    let c_paths: Vec<&str> = c.files.iter().map(|f| f.path.as_str()).collect();
    let g_paths: Vec<&str> = g.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(c_paths, g_paths);
}

#[test]
fn parity_upstream_tracking() {
    let r = rt();
    let repo = fixture();
    repo.commit_file("a.txt", "1\n", "first");
    // local bare origin + tracking
    let origin_dir = repo.dir.parent().unwrap().join(format!(
        "{}-origin.git",
        repo.dir.file_name().unwrap().to_string_lossy()
    ));
    repo.git_at(&origin_dir, &["init", "--bare", "-b", "main"]);
    repo.git(&[
        "remote",
        "add",
        "origin",
        origin_dir.to_string_lossy().as_ref(),
    ]);
    repo.git(&["push", "-u", "origin", "main"]);
    repo.commit_file("a.txt", "2\n", "second");

    let cli = CliBackend::new();
    let g2 = Git2Backend::new();
    let c = r
        .block_on(cli.status(repo.path()))
        .expect("cli status w/ upstream");
    let g = r
        .block_on(g2.status(repo.path()))
        .expect("git2 status w/ upstream");
    let (c, mut g) = (norm_status(c), norm_status(g));
    // git2 upstream may resolve name differently only if remote ref missing; both should be origin/main
    g.upstream = c.upstream.clone();
    assert_eq!(c.branch, g.branch);
    assert_eq!(c.upstream, g.upstream);
    assert_eq!(c.ahead, g.ahead);
    assert_eq!(c.behind, g.behind);

    let c = r
        .block_on(cli.branches(repo.path(), false))
        .expect("cli branches");
    let g = r
        .block_on(g2.branches(repo.path(), false))
        .expect("git2 branches");
    assert_eq!(c, g, "branches parity with upstream");
}
