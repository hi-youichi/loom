//! Low-level git worktree command wrappers.
//!
//! Uses `std::process::Command` to call system `git` (no `git2` C binding dependency).
//! All commands include index-lock retry with exponential backoff.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Errors from git worktree operations.
#[derive(Debug, thiserror::Error)]
pub enum GitWorktreeError {
    #[error("git command failed: {message}\nstdout: {stdout}\nstderr: {stderr}")]
    CommandFailed {
        message: String,
        stdout: String,
        stderr: String,
    },
    #[error("not a git repository: {path}")]
    NotGitRepo { path: PathBuf },
    #[error("git not found on PATH")]
    GitNotFound,
    #[error("worktree already exists: {path}")]
    WorktreeExists { path: PathBuf },
    #[error("branch already exists: {name}")]
    BranchExists { name: String },
    #[error("index lock conflict after retries")]
    IndexLockConflict,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, GitWorktreeError>;

/// Run a git command in the given directory, returning stdout.
pub(crate) fn run_git(workdir: &Path, args: &[&str]) -> Result<std::process::Output> {
    let mut git_cmd = std::process::Command::new("git");
    git_cmd.current_dir(workdir).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        git_cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = git_cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GitWorktreeError::GitNotFound
        } else {
            GitWorktreeError::Io(e)
        }
    })?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(GitWorktreeError::CommandFailed {
            message: format!("git {} (in {})", args.join(" "), workdir.display()),
            stdout,
            stderr,
        });
    }
    Ok(output)
}

/// Check if an error is caused by an index lock.
fn is_index_lock_error(err: &GitWorktreeError) -> bool {
    match err {
        GitWorktreeError::CommandFailed { stderr, .. } => {
            stderr.contains("index.lock") || stderr.contains("Unable to create")
        }
        _ => false,
    }
}

/// Run a git command with index-lock retry (exponential backoff: 100ms, 200ms, 400ms).
pub async fn run_git_with_retry(workdir: &Path, args: &[&str]) -> Result<std::process::Output> {
    let mut delay = Duration::from_millis(100);
    for attempt in 0..3 {
        match run_git(workdir, args) {
            Ok(output) => return Ok(output),
            Err(e) if is_index_lock_error(&e) && attempt < 2 => {
                tracing::warn!(
                    "git index lock conflict, retrying in {:?} (attempt {}/3)",
                    delay,
                    attempt + 1
                );
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
            Err(e) => return Err(e),
        }
    }
    Err(GitWorktreeError::IndexLockConflict)
}

/// Resolve the git repository root from a working directory.
pub fn resolve_repo_root(workdir: &Path) -> Result<PathBuf> {
    let output = run_git(workdir, &["rev-parse", "--show-toplevel"])?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let root = PathBuf::from(path);
    if !root.exists() {
        return Err(GitWorktreeError::NotGitRepo {
            path: workdir.to_path_buf(),
        });
    }
    Ok(root)
}

/// Check if a directory is inside a git repository.
pub fn is_git_repo(path: &Path) -> bool {
    resolve_repo_root(path).is_ok()
}

/// Create a new git worktree.
///
/// If `branch_name` is Some, creates a new branch with that name.
/// If `detached` is true, creates a detached HEAD worktree.
pub fn worktree_add(
    repo_root: &Path,
    target_path: &Path,
    branch_name: Option<&str>,
    detached: bool,
    base_ref: &str,
) -> Result<()> {
    let mut args = vec!["worktree", "add"];

    if detached {
        args.push("--detach");
    } else if let Some(branch) = branch_name {
        args.push("-b");
        // Safe: branch name is generated internally
        let b = branch.to_string();
        args.push(Box::leak(b.into_boxed_str()));
    }

    // Target path
    let tp = target_path.to_string_lossy().to_string();
    args.push(Box::leak(tp.into_boxed_str()));

    // Base ref (last positional arg)
    let br = base_ref.to_string();
    args.push(Box::leak(br.into_boxed_str()));

    run_git(repo_root, &args)?;
    Ok(())
}

/// Remove a git worktree.
pub fn worktree_remove(path: &Path, force: bool) -> Result<()> {
    let repo_root = resolve_repo_root(path).unwrap_or_else(|_| path.to_path_buf());
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    let p = path.to_string_lossy().to_string();
    args.push(Box::leak(p.into_boxed_str()));
    run_git(&repo_root, &args)?;
    Ok(())
}

/// List all worktree paths for a repository.
pub fn worktree_list(repo_root: &Path) -> Result<Vec<PathBuf>> {
    let output = run_git(repo_root, &["worktree", "list", "--porcelain"])?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut paths = Vec::new();
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            paths.push(PathBuf::from(p));
        }
    }
    Ok(paths)
}

/// Delete a branch.
pub fn branch_delete(repo_root: &Path, branch: &str) -> Result<()> {
    run_git(repo_root, &["branch", "-D", branch])?;
    Ok(())
}

/// Check if a working directory has uncommitted changes.
pub fn has_uncommitted_changes(worktree_path: &Path) -> Result<bool> {
    let output = run_git(worktree_path, &["status", "--porcelain"])?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(!text.trim().is_empty())
}

/// Get the diff between a worktree and a base ref.
pub fn diff_worktree(worktree_path: &Path, base: &str) -> Result<String> {
    let output = run_git(worktree_path, &["diff", base])?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get the current branch name.
pub fn current_branch(path: &Path) -> Result<String> {
    let output = run_git(path, &["branch", "--show-current"])?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Resolve the default ref (HEAD commit hash).
pub fn resolve_default_ref(repo_root: &Path) -> Result<String> {
    let output = run_git(repo_root, &["rev-parse", "HEAD"])?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get list of changed files (relative paths).
pub fn changed_files(worktree_path: &Path) -> Result<Vec<String>> {
    let output = run_git(worktree_path, &["diff", "--name-only", "HEAD"])?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().map(|l| l.to_string()).collect())
}

/// Enable sparse checkout for a worktree (requires git >= 2.25).
pub fn enable_sparse_checkout(worktree_path: &Path, paths: &[String]) -> Result<()> {
    run_git(worktree_path, &["sparse-checkout", "init", "--cone"])?;
    let mut args = vec!["sparse-checkout", "set"];
    let owned: Vec<String> = paths.iter().map(|s| s.as_str().to_string()).collect();
    for p in &owned {
        args.push(Box::leak(p.clone().into_boxed_str()));
    }
    run_git(worktree_path, &args)?;
    Ok(())
}

/// Add a relative `storage_path` entry to `.gitignore` if not already present.
///
/// No-op when `storage_path` is outside `repo_root` (e.g. the default
/// `<repo_parent>/worktrees/<repo_name>/` location), since git cannot see those
/// directories anyway and an entry would be confusing.
pub fn ensure_gitignore_entry(repo_root: &Path, storage_path: &Path) -> Result<()> {
    let rel = match storage_path.strip_prefix(repo_root) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    let entry = format!("{}/", rel.display());
    let gitignore = repo_root.join(".gitignore");
    if gitignore.exists() {
        let content = std::fs::read_to_string(&gitignore)?;
        if content.lines().any(|l| l.trim() == entry) {
            return Ok(());
        }
        // Append
        let mut content = content;
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&entry);
        content.push('\n');
        std::fs::write(&gitignore, content)?;
    } else {
        std::fs::write(&gitignore, format!("{}\n", entry))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_worktree_error_command_failed() {
        let err = GitWorktreeError::CommandFailed {
            message: "test error".to_string(),
            stdout: "test stdout".to_string(),
            stderr: "test stderr".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "git command failed: test error\nstdout: test stdout\nstderr: test stderr"
        );
    }

    #[test]
    fn git_worktree_error_not_git_repo() {
        let path = PathBuf::from("/test/path");
        let err = GitWorktreeError::NotGitRepo { path: path.clone() };
        assert_eq!(err.to_string(), "not a git repository: /test/path");
    }

    #[test]
    fn git_worktree_error_git_not_found() {
        let err = GitWorktreeError::GitNotFound;
        assert_eq!(err.to_string(), "git not found on PATH");
    }

    #[test]
    fn git_worktree_error_worktree_exists() {
        let path = PathBuf::from("/test/worktree");
        let err = GitWorktreeError::WorktreeExists { path: path.clone() };
        assert_eq!(err.to_string(), "worktree already exists: /test/worktree");
    }

    #[test]
    fn git_worktree_error_branch_exists() {
        let err = GitWorktreeError::BranchExists {
            name: "test-branch".to_string(),
        };
        assert_eq!(err.to_string(), "branch already exists: test-branch");
    }

    #[test]
    fn git_worktree_error_index_lock_conflict() {
        let err = GitWorktreeError::IndexLockConflict;
        assert_eq!(err.to_string(), "index lock conflict after retries");
    }

    #[test]
    fn git_worktree_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
        let git_err = GitWorktreeError::from(io_err);
        assert!(git_err.to_string().contains("IO error"));
    }

    #[test]
    fn is_index_lock_error_detects_lock_conflict() {
        let err = GitWorktreeError::CommandFailed {
            message: "git error".to_string(),
            stdout: "".to_string(),
            stderr: "index.lock: File exists".to_string(),
        };
        assert!(is_index_lock_error(&err));
    }

    #[test]
    fn is_index_lock_error_detects_unable_to_create() {
        let err = GitWorktreeError::CommandFailed {
            message: "git error".to_string(),
            stdout: "".to_string(),
            stderr: "Unable to create 'index.lock'".to_string(),
        };
        assert!(is_index_lock_error(&err));
    }

    #[test]
    fn is_index_lock_error_non_command_errors() {
        let err = GitWorktreeError::NotGitRepo {
            path: PathBuf::from("/test"),
        };
        assert!(!is_index_lock_error(&err));
    }

    #[test]
    fn is_index_lock_error_no_lock_in_stderr() {
        let err = GitWorktreeError::CommandFailed {
            message: "git error".to_string(),
            stdout: "".to_string(),
            stderr: "some other error".to_string(),
        };
        assert!(!is_index_lock_error(&err));
    }

    #[test]
    fn git_worktree_error_display_formats_correctly() {
        let errors = vec![
            GitWorktreeError::GitNotFound,
            GitWorktreeError::IndexLockConflict,
            GitWorktreeError::BranchExists {
                name: "main".to_string(),
            },
        ];

        for err in errors {
            let display_str = err.to_string();
            assert!(!display_str.is_empty());
        }
    }
}
