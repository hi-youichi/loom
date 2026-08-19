//! Error-path coverage: invalid inputs must map to typed errors on both
//! backends (NotFound / InvalidParams / Conflict).

use loom_git::cli::CliBackend;
use loom_git::git2_backend::Git2Backend;
use loom_git::GitBackend;

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
fn stage_missing_file_is_not_found() {
    let r = rt();
    for name in ["cli", "git2"] {
        let repo = crate::common::FixtureRepo::new("stagemissing");
        repo.commit_file("a.txt", "x\n", "c");
        let b = backend(name);
        let err = r
            .block_on(b.stage_file(repo.path(), "does-not-exist.txt"))
            .unwrap_err();
        assert!(
            matches!(err.kind(), loom_git::GitErrorKind::NotFound),
            "({name}) stage missing file must be NotFound, got {err:?}"
        );
    }
}

#[test]
fn checkout_missing_branch_is_not_found() {
    let r = rt();
    for name in ["cli", "git2"] {
        let repo = crate::common::FixtureRepo::new("checkoutmissing");
        repo.commit_file("a.txt", "x\n", "c");
        let b = backend(name);
        let err = r
            .block_on(b.checkout_branch(repo.path(), "nope"))
            .unwrap_err();
        assert!(
            matches!(err.kind(), loom_git::GitErrorKind::NotFound),
            "({name}) checkout missing branch must be NotFound, got {err:?}"
        );
    }
}

#[test]
fn reset_missing_commit_is_not_found() {
    let r = rt();
    for name in ["cli", "git2"] {
        let repo = crate::common::FixtureRepo::new("resetmissing");
        repo.commit_file("a.txt", "x\n", "c");
        let b = backend(name);
        let err = r
            .block_on(b.reset_to_commit(repo.path(), "deadbeefdeadbeef", "hard"))
            .unwrap_err();
        assert!(
            matches!(err.kind(), loom_git::GitErrorKind::NotFound),
            "({name}) reset missing commit must be NotFound, got {err:?}"
        );
    }
}

#[test]
fn reset_unknown_mode_is_invalid_params() {
    let r = rt();
    for name in ["cli", "git2"] {
        let repo = crate::common::FixtureRepo::new("resetmode");
        repo.commit_file("a.txt", "x\n", "c");
        let b = backend(name);
        let err = r
            .block_on(b.reset_to_commit(repo.path(), "HEAD", "bogus"))
            .unwrap_err();
        assert!(
            matches!(err.kind(), loom_git::GitErrorKind::InvalidParams),
            "({name}) reset bogus mode must be InvalidParams, got {err:?}"
        );
    }
}

#[test]
fn commit_without_identity_is_invalid_params() {
    let r = rt();
    for name in ["cli", "git2"] {
        let repo = crate::common::FixtureRepo::new("noidentity");
        repo.commit_file("a.txt", "x\n", "c");
        // strip identity
        repo.git(&["config", "--unset", "user.name"]);
        repo.git(&["config", "--unset", "user.email"]);
        let b = backend(name);
        let err = r
            .block_on(b.commit(
                repo.path(),
                loom_git::types::CommitRequest {
                    message: "no identity".into(),
                    amend: false,
                    signoff: false,
                },
            ))
            .unwrap_err();
        assert!(
            matches!(err.kind(), loom_git::GitErrorKind::InvalidParams),
            "({name}) commit without identity must be InvalidParams, got {err:?}"
        );
    }
}

#[test]
fn apply_bad_patch_is_invalid_params() {
    let r = rt();
    for name in ["cli", "git2"] {
        let repo = crate::common::FixtureRepo::new("badpatch");
        repo.commit_file("a.txt", "x\n", "c");
        let b = backend(name);
        let err = r
            .block_on(b.stage_hunk(repo.path(), "this is not a diff at all"))
            .unwrap_err();
        assert!(
            matches!(err.kind(), loom_git::GitErrorKind::InvalidParams),
            "({name}) malformed patch must be InvalidParams, got {err:?}"
        );
    }
}

#[test]
fn commit_no_staged_changes_is_invalid_params() {
    let r = rt();
    for name in ["cli", "git2"] {
        let repo = crate::common::FixtureRepo::new("nostaged");
        repo.commit_file("a.txt", "x\n", "c");
        let b = backend(name);
        let err = r
            .block_on(b.commit(
                repo.path(),
                loom_git::types::CommitRequest {
                    message: "empty".into(),
                    amend: false,
                    signoff: false,
                },
            ))
            .unwrap_err();
        assert!(
            matches!(err.kind(), loom_git::GitErrorKind::InvalidParams),
            "({name}) commit without staged changes must be InvalidParams, got {err:?}"
        );
    }
}
