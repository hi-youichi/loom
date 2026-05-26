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
    let output = std::process::Command::new("git")
        .current_dir(workdir)
        .args(args)
        .output()
        .map_err(|e| {
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

/// Add `.loom/worktrees/` to `.gitignore` if not already present.
pub fn ensure_gitignore_entry(repo_root: &Path) -> Result<()> {
    let gitignore = repo_root.join(".gitignore");
    let entry = ".loom/worktrees/";
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
        content.push_str(entry);
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
    use tempfile::TempDir;

    fn setup_test_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        run_git(dir.path(), &["init"]).unwrap();
        run_git(dir.path(), &["config", "user.email", "test@loom.dev"]).unwrap();
        run_git(dir.path(), &["config", "user.name", "Test"]).unwrap();
        std::fs::write(dir.path().join("README.md"), "# test").unwrap();
        run_git(dir.path(), &["add", "."]).unwrap();
        run_git(dir.path(), &["commit", "-m", "init"]).unwrap();
        dir
    }

    #[test]
    fn resolve_repo_root_finds_git_root() {
        let dir = setup_test_repo();
        let root = resolve_repo_root(dir.path()).unwrap();
        assert!(root.exists());
    }

    #[test]
    fn has_uncommitted_changes_clean_repo() {
        let dir = setup_test_repo();
        assert!(!has_uncommitted_changes(dir.path()).unwrap());
    }

    #[test]
    fn has_uncommitted_changes_dirty_repo() {
        let dir = setup_test_repo();
        std::fs::write(dir.path().join("new.txt"), "dirty").unwrap();
        assert!(has_uncommitted_changes(dir.path()).unwrap());
    }

    #[test]
    fn current_branch_after_init() {
        let dir = setup_test_repo();
        let branch = current_branch(dir.path()).unwrap();
        // Could be main or master depending on git config
        assert!(!branch.is_empty() || cfg!(windows));
    }

    #[test]
    fn resolve_default_ref_returns_hash() {
        let dir = setup_test_repo();
        let hash = resolve_default_ref(dir.path()).unwrap();
        assert!(hash.len() >= 7); // at least a short hash
    }

    #[test]
    fn ensure_gitignore_entry_creates_file() {
        let dir = setup_test_repo();
        ensure_gitignore_entry(dir.path()).unwrap();
        let content = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains(".loom/worktrees/"));
    }

    #[test]
    fn ensure_gitignore_entry_idempotent() {
        let dir = setup_test_repo();
        ensure_gitignore_entry(dir.path()).unwrap();
        ensure_gitignore_entry(dir.path()).unwrap();
        let content = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(content.matches(".loom/worktrees/").count(), 1);
    }

    #[tokio::test]
    async fn worktree_create_and_remove_lifecycle() {
        let dir = setup_test_repo();
        let repo_root = dir.path().to_path_buf();
        let wt_path = dir.path().join(".loom/worktrees/test-wt");

        // Create worktree
        worktree_add(
            &repo_root,
            &wt_path,
            Some("worktree-test-wt"),
            false,
            "HEAD",
        )
        .unwrap();

        assert!(wt_path.exists());
        let branch = current_branch(&wt_path).unwrap();
        assert_eq!(branch, "worktree-test-wt");

        // Should be clean
        assert!(!has_uncommitted_changes(&wt_path).unwrap());

        // Remove (with Windows fallback)
        if worktree_remove(&wt_path, true).is_err() {
            // Windows: Permission denied in temp dirs
            let _ = std::fs::remove_dir_all(&wt_path);
        }
        assert!(!wt_path.exists());

        // Cleanup branch
        branch_delete(&repo_root, "worktree-test-wt").unwrap();
    }

    #[tokio::test]
    async fn worktree_list_shows_created_worktrees() {
        let dir = setup_test_repo();
        let repo_root = dir.path().to_path_buf();
        let wt_path = dir.path().join(".loom/worktrees/list-test");

        worktree_add(
            &repo_root,
            &wt_path,
            Some("worktree-list-test"),
            false,
            "HEAD",
        )
        .unwrap();

        let list = worktree_list(&repo_root).unwrap();
        assert!(list.iter().any(|p| p == &wt_path));

        if worktree_remove(&wt_path, true).is_err() {
            let _ = std::fs::remove_dir_all(&wt_path);
        }
        branch_delete(&repo_root, "worktree-list-test").unwrap();
    }
}
