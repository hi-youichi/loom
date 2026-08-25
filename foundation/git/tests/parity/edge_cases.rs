//! Edge cases that stress error/detached/unborn branches and the force-push
//! and pull.rebase paths on both backends.

use anureo_git::cli::CliBackend;
use anureo_git::git2_backend::Git2Backend;
use anureo_git::GitBackend;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

fn backend(name: &str) -> Box<dyn GitBackend> {
    match name {
        "cli" => Box::new(CliBackend::new()),
        _ => Box::new(Git2Backend::new()),
    }
}

#[test]
fn empty_repo_log_is_not_found() {
    let r = rt();
    for name in ["cli", "git2"] {
        let repo = crate::common::FixtureRepo::new("emptylog");
        let b = backend(name);
        let err = r
            .block_on(b.log(
                repo.path(),
                &anureo_git::backend::LogQuery {
                    limit: 10,
                    skip: 0,
                    branch: None,
                    file_path: None,
                },
            ))
            .unwrap_err();
        assert!(
            matches!(err.kind(), anureo_git::GitErrorKind::NotFound),
            "({name}) empty repo log must be NotFound, got {err:?}"
        );
    }
}

#[test]
fn detached_head_status_reports_detached() {
    let r = rt();
    for name in ["cli", "git2"] {
        let repo = crate::common::FixtureRepo::new("detached");
        repo.commit_file("a.txt", "x\n", "c");
        repo.git(&["checkout", "--detach"]);
        let b = backend(name);
        let st = r.block_on(b.status(repo.path())).unwrap();
        assert_eq!(st.branch, "(detached)", "({name}) detached branch name");
    }
}

#[test]
fn unborn_head_status_reports_unborn() {
    let r = rt();
    for name in ["cli", "git2"] {
        let repo = crate::common::FixtureRepo::new("unborn");
        let b = backend(name);
        let st = r.block_on(b.status(repo.path())).unwrap();
        assert!(
            st.branch == "(unborn)" || st.branch == "main",
            "({name}) unborn branch marker, got {}",
            st.branch
        );
    }
}

#[test]
fn push_non_fast_forward_rejected_then_forced() {
    let r = rt();
    for name in ["cli", "git2"] {
        let repo = crate::common::FixtureRepo::new("forcepush");
        repo.commit_file("a.txt", "1\n", "first");
        let origin = repo.dir.parent().unwrap().join(format!(
            "{}-origin.git",
            repo.dir.file_name().unwrap().to_string_lossy()
        ));
        repo.git_at(&origin, &["init", "--bare", "-b", "main"]);
        repo.git(&["remote", "add", "origin", origin.to_string_lossy().as_ref()]);
        repo.git(&["push", "-u", "origin", "main"]);

        let other = repo.dir.parent().unwrap().join(format!(
            "{}-other",
            repo.dir.file_name().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&other).unwrap();
        repo.git_at(&other, &["clone", origin.to_string_lossy().as_ref(), "."]);
        repo.git_at(&other, &["config", "user.name", "T"]);
        repo.git_at(&other, &["config", "user.email", "t@e.com"]);
        std::fs::write(other.join("b.txt"), "remote\n").unwrap();
        repo.git_at(&other, &["add", "."]);
        repo.git_at(&other, &["commit", "-m", "advance"]);
        repo.git_at(&other, &["push", "origin", "main"]);

        let b = backend(name);
        // repo is behind origin/main now; non-forced push must be rejected
        let rejected = r
            .block_on(b.push(repo.path(), "origin", "main", false, false))
            .unwrap_err();
        assert!(
            !matches!(rejected.kind(), anureo_git::GitErrorKind::NotFound),
            "({name}) non-ff push must fail, got {rejected:?}"
        );

        // diverge locally, then force
        std::fs::write(repo.dir.join("c.txt"), "local\n").unwrap();
        repo.git(&["add", "."]);
        repo.git(&["commit", "-m", "local"]);
        let forced = r
            .block_on(b.push(repo.path(), "origin", "main", true, false))
            .expect("forced push");
        assert!(!forced.remote_sha.is_empty());
    }
}

#[test]
fn pull_respects_rebase_config() {
    let r = rt();
    for name in ["cli", "git2"] {
        let repo = crate::common::FixtureRepo::new("pullrebase");
        repo.commit_file("a.txt", "1\n", "first");
        let origin = repo.dir.parent().unwrap().join(format!(
            "{}-origin.git",
            repo.dir.file_name().unwrap().to_string_lossy()
        ));
        repo.git_at(&origin, &["init", "--bare", "-b", "main"]);
        repo.git(&["remote", "add", "origin", origin.to_string_lossy().as_ref()]);
        repo.git(&["push", "-u", "origin", "main"]);
        repo.git(&["config", "pull.rebase", "true"]);

        let other = repo.dir.parent().unwrap().join(format!(
            "{}-other",
            repo.dir.file_name().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&other).unwrap();
        repo.git_at(&other, &["clone", origin.to_string_lossy().as_ref(), "."]);
        repo.git_at(&other, &["config", "user.name", "T"]);
        repo.git_at(&other, &["config", "user.email", "t@e.com"]);
        std::fs::write(other.join("b.txt"), "remote\n").unwrap();
        repo.git_at(&other, &["add", "."]);
        repo.git_at(&other, &["commit", "-m", "advance"]);
        repo.git_at(&other, &["push", "origin", "main"]);

        // diverge locally so a plain ff cannot happen
        std::fs::write(repo.dir.join("a.txt"), "2\n").unwrap();
        repo.git(&["add", "."]);
        repo.git(&["commit", "-m", "local"]);

        let b = backend(name);
        let result = r
            .block_on(b.pull(repo.path(), "origin", Some("main")))
            .expect("pull with rebase config");
        // rebase applied: local commit replayed on top of remote
        let log = r
            .block_on(b.log(
                repo.path(),
                &anureo_git::backend::LogQuery {
                    limit: 5,
                    skip: 0,
                    branch: None,
                    file_path: None,
                },
            ))
            .unwrap();
        assert_eq!(
            log[0].message, "local",
            "({name}) rebased local commit first"
        );
        let _ = result;
    }
}
