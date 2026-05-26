//! WorktreeManager — high-level worktree lifecycle management.

use std::path::{Path, PathBuf};

use super::git_ops;
use super::{
    sanitize_slug, BaseRef, CleanupStrategy, ConflictDetection, ConflictInfo, ConflictSeverity,
    WorktreeConfig, WorktreeHandle, WorktreeState,
};

/// Errors from worktree manager operations.
#[derive(Debug, thiserror::Error)]
pub enum WorktreeManagerError {
    #[error("git operation failed: {0}")]
    Git(#[from] git_ops::GitWorktreeError),
    #[error("not a git repository (or git not installed): {path}")]
    NotGitRepo { path: PathBuf },
    #[error("worktree nesting detected — already inside a Loom worktree")]
    NestedWorktree,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, WorktreeManagerError>;

/// Manages git worktree lifecycle for agent isolation.
pub struct WorktreeManager {
    /// Root of the main git repository (resolved from working dir).
    repo_root: PathBuf,
    /// Configuration for worktree creation.
    config: WorktreeConfig,
}

impl WorktreeManager {
    /// Create a new manager by resolving the git repo root from the working directory.
    pub fn from_working_dir(working_dir: &Path, config: WorktreeConfig) -> Result<Self> {
        let repo_root = git_ops::resolve_repo_root(working_dir).map_err(|_| {
            WorktreeManagerError::NotGitRepo {
                path: working_dir.to_path_buf(),
            }
        })?;
        Ok(Self { repo_root, config })
    }

    /// Create a manager with a known repo root (for testing).
    pub fn new(repo_root: PathBuf, config: WorktreeConfig) -> Self {
        Self { repo_root, config }
    }

    /// Get the default storage directory for worktrees.
    fn storage_path(&self) -> PathBuf {
        self.config
            .storage_dir
            .clone()
            .unwrap_or_else(|| self.repo_root.join(".loom/worktrees"))
    }

    /// Create an isolated worktree for a sub-agent.
    ///
    /// This performs the full lifecycle:
    /// 1. Resolve base ref
    /// 2. Create storage directory
    /// 3. `git worktree add` with a new branch
    /// 4. Optionally enable sparse checkout
    /// 5. Ensure `.gitignore` entry
    pub async fn create_for_agent(
        &self,
        agent_name: &str,
        task_hint: Option<&str>,
        estimated_paths: Option<&[String]>,
    ) -> Result<WorktreeHandle> {
        // Generate slug
        let slug = match task_hint {
            Some(hint) => format!("{}-{}", sanitize_slug(agent_name), sanitize_slug(hint)),
            None => sanitize_slug(agent_name),
        };
        let branch_name = format!(
            "{}{}",
            self.config
                .branch_prefix
                .clone()
                .unwrap_or_else(|| "worktree-".to_string()),
            slug
        );
        let storage = self.storage_path();
        let target = storage.join(&slug);

        // Ensure storage dir exists
        std::fs::create_dir_all(&storage)?;

        // Resolve base ref
        let base_ref = match &self.config.base_ref {
            BaseRef::Fresh | BaseRef::Head => "HEAD".to_string(),
            BaseRef::Ref(r) => r.clone(),
        };

        // Create worktree
        git_ops::worktree_add(
            &self.repo_root,
            &target,
            Some(&branch_name),
            self.config.detached,
            &base_ref,
        )?;

        // Sparse checkout if configured
        if !self.config.sparse_paths.is_empty() {
            if let Err(e) = git_ops::enable_sparse_checkout(&target, &self.config.sparse_paths) {
                tracing::warn!("sparse checkout failed (needs git >= 2.25): {}", e);
            }
        }

        // Ensure .gitignore entry
        let _ = git_ops::ensure_gitignore_entry(&self.repo_root);

        tracing::info!(
            agent = %agent_name,
            worktree_path = %target.display(),
            branch = %branch_name,
            "Created worktree for sub-agent"
        );

        Ok(WorktreeHandle {
            repo_root: self.repo_root.clone(),
            path: target,
            branch: Some(branch_name),
            has_changes: false,
            agent_name: agent_name.to_string(),
            estimated_paths: estimated_paths
                .map(|p| p.to_vec())
                .unwrap_or_default(),
            state: WorktreeState::Active,
        })
    }

    /// List all active worktrees under `.loom/worktrees/`.
    pub fn list_active(&self) -> Result<Vec<WorktreeHandle>> {
        let storage = self.storage_path();
        if !storage.exists() {
            return Ok(Vec::new());
        }

        let mut handles = Vec::new();
        let entries = std::fs::read_dir(&storage)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            // Skip trash dir
            if path
                .file_name()
                .is_some_and(|n| n == ".trash")
            {
                continue;
            }
            let branch = git_ops::current_branch(&path).ok();
            let has_changes = git_ops::has_uncommitted_changes(&path).unwrap_or(false);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            handles.push(WorktreeHandle {
                repo_root: self.repo_root.clone(),
                path,
                branch,
                has_changes,
                agent_name: name.clone(),
                estimated_paths: Vec::new(),
                state: if has_changes {
                    WorktreeState::Completed
                } else {
                    WorktreeState::Active
                },
            });
        }
        Ok(handles)
    }

    /// Check if a handle's worktree has uncommitted changes.
    pub async fn check_changes(&self, handle: &WorktreeHandle) -> Result<bool> {
        Ok(git_ops::has_uncommitted_changes(&handle.path)?)
    }

    /// Get diff between worktree and base ref.
    pub async fn diff_main(&self, handle: &WorktreeHandle) -> Result<String> {
        let base = match &handle.branch {
            Some(_) => "HEAD".to_string(),
            None => "HEAD".to_string(),
        };
        Ok(git_ops::diff_worktree(&handle.path, &base)?)
    }

    /// Get list of changed files in a worktree.
    pub async fn changed_files(&self, handle: &WorktreeHandle) -> Result<Vec<String>> {
        Ok(git_ops::changed_files(&handle.path)?)
    }

    /// Clean up a worktree (remove directory + branch).
    pub async fn cleanup(&self, handle: WorktreeHandle) -> Result<()> {
        match self.config.cleanup_strategy {
            CleanupStrategy::Sync => {
                self.cleanup_sync(&handle)?;
            }
            CleanupStrategy::AsyncTrash => {
                self.cleanup_async_trash(handle)?;
            }
        }
        Ok(())
    }

    fn cleanup_sync(&self, handle: &WorktreeHandle) -> Result<()> {
        let _ = git_ops::worktree_remove(&handle.path, true);
        if let Some(ref branch) = handle.branch {
            let _ = git_ops::branch_delete(&self.repo_root, branch);
        }
        tracing::info!(
            path = %handle.path.display(),
            branch = ?handle.branch,
            "Worktree cleaned up (sync)"
        );
        Ok(())
    }

    fn cleanup_async_trash(&self, handle: WorktreeHandle) -> Result<()> {
        let trash_dir = self.storage_path().join(".trash");
        std::fs::create_dir_all(&trash_dir)?;

        let file_name = handle
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let dest = trash_dir.join(&file_name);

        // Move to trash (may fail across drives on Windows)
        if std::fs::rename(&handle.path, &dest).is_err() {
            // Fallback: copy + delete
            if let Err(e) = copy_dir_recursive(&handle.path, &dest) {
                tracing::warn!("Failed to move worktree to trash: {}", e);
                // Last resort: direct removal
                let _ = git_ops::worktree_remove(&handle.path, true);
                if let Some(ref branch) = handle.branch {
                    let _ = git_ops::branch_delete(&self.repo_root, branch);
                }
                return Ok(());
            }
            let _ = std::fs::remove_dir_all(&handle.path);
        }

        // Background prune
        let repo_root = self.repo_root.clone();
        let branch = handle.branch.clone();
        let dest_clone = dest.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let _ = git_ops::worktree_remove(&dest_clone, true);
            if let Some(ref b) = branch {
                let _ = git_ops::branch_delete(&repo_root, b);
            }
            let _ = std::fs::remove_dir_all(&dest_clone);
        });

        tracing::info!(
            path = %handle.path.display(),
            "Worktree moved to trash, will be pruned in background"
        );
        Ok(())
    }

    /// Clean up stale (no changes, not active) worktrees.
    pub async fn cleanup_stale(&self) -> Result<usize> {
        let active = self.list_active()?;
        let mut cleaned = 0;
        for handle in active {
            if !handle.has_changes {
                self.cleanup(handle).await?;
                cleaned += 1;
            }
        }
        Ok(cleaned)
    }

    /// Prune the `.trash/` directory.
    pub async fn prune_trash(&self) -> Result<usize> {
        let trash_dir = self.storage_path().join(".trash");
        if !trash_dir.exists() {
            return Ok(0);
        }
        let mut pruned = 0;
        let entries = std::fs::read_dir(&trash_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let _ = git_ops::worktree_remove(&path, true);
                let _ = std::fs::remove_dir_all(&path);
                pruned += 1;
            }
        }
        Ok(pruned)
    }

    /// Detect file-path level conflicts between worktree handles.
    pub fn detect_parallel_conflicts(
        &self,
        handles: &[&WorktreeHandle],
    ) -> Vec<ConflictInfo> {
        if !matches!(self.config.conflict_detection, ConflictDetection::FilePath | ConflictDetection::HunkLevel) {
            return Vec::new();
        }

        let mut conflicts = Vec::new();
        for i in 0..handles.len() {
            for j in (i + 1)..handles.len() {
                let a = &handles[i];
                let b = &handles[j];
                let overlap: Vec<String> = a
                    .estimated_paths
                    .iter()
                    .filter(|p| {
                        b.estimated_paths
                            .iter()
                            .any(|q: &String| p.starts_with(q.as_str()) || q.starts_with(p.as_str()))
                    })
                    .cloned()
                    .collect();
                if !overlap.is_empty() {
                    conflicts.push(ConflictInfo {
                        other_agent: b.agent_name.clone(),
                        conflicting_paths: overlap,
                        severity: ConflictSeverity::FileOverlap,
                    });
                }
            }
        }
        conflicts
    }

    /// Get or create a worktree — reuse existing if it exists and is valid.
    pub async fn get_or_create(
        &self,
        slug: &str,
        agent_name: &str,
        estimated_paths: Option<&[String]>,
    ) -> Result<WorktreeHandle> {
        let storage = self.storage_path();
        let target = storage.join(slug);

        if target.exists() {
            // Validate existing worktree
            if let Ok(branch) = git_ops::current_branch(&target) {
                let has_changes = git_ops::has_uncommitted_changes(&target).unwrap_or(false);
                return Ok(WorktreeHandle {
                    repo_root: self.repo_root.clone(),
                    path: target,
                    branch: Some(branch),
                    has_changes,
                    agent_name: agent_name.to_string(),
                    estimated_paths: estimated_paths
                        .map(|p| p.to_vec())
                        .unwrap_or_default(),
                    state: WorktreeState::Active,
                });
            }
            // Invalid state, clean up and recreate
            let _ = git_ops::worktree_remove(&target, true);
            let _ = std::fs::remove_dir_all(&target);
        }

        // Use slug directly as the worktree directory name
        let branch_name = format!(
            "{}{}",
            self.config
                .branch_prefix
                .clone()
                .unwrap_or_else(|| "worktree-".to_string()),
            slug
        );
        let storage = self.storage_path();
        let target = storage.join(slug);
        std::fs::create_dir_all(&storage)?;

        let base_ref = match &self.config.base_ref {
            BaseRef::Fresh | BaseRef::Head => "HEAD".to_string(),
            BaseRef::Ref(r) => r.clone(),
        };

        git_ops::worktree_add(
            &self.repo_root,
            &target,
            Some(&branch_name),
            self.config.detached,
            &base_ref,
        )?;

        if !self.config.sparse_paths.is_empty() {
            if let Err(e) = git_ops::enable_sparse_checkout(&target, &self.config.sparse_paths) {
                tracing::warn!("sparse checkout failed: {}", e);
            }
        }

        let _ = git_ops::ensure_gitignore_entry(&self.repo_root);

        Ok(WorktreeHandle {
            repo_root: self.repo_root.clone(),
            path: target,
            branch: Some(branch_name),
            has_changes: false,
            agent_name: agent_name.to_string(),
            estimated_paths: estimated_paths
                .map(|p| p.to_vec())
                .unwrap_or_default(),
            state: WorktreeState::Active,
        })
    }
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        git_ops::run_git(dir.path(), &["init"]).unwrap();
        git_ops::run_git(dir.path(), &["config", "user.email", "test@loom.dev"]).unwrap();
        git_ops::run_git(dir.path(), &["config", "user.name", "Test"]).unwrap();
        std::fs::write(dir.path().join("README.md"), "# test").unwrap();
        git_ops::run_git(dir.path(), &["add", "."]).unwrap();
        git_ops::run_git(dir.path(), &["commit", "-m", "init"]).unwrap();
        dir
    }

    #[tokio::test]
    async fn manager_creates_and_lists_worktrees() {
        let dir = setup_test_repo();
        let config = WorktreeConfig::default();
        let manager = WorktreeManager::new(dir.path().to_path_buf(), config);

        let handle = manager
            .create_for_agent("dev", Some("auth"), None)
            .await
            .unwrap();
        assert!(handle.path.exists());
        assert_eq!(handle.branch.as_deref(), Some("worktree-dev-auth"));

        let list = manager.list_active().unwrap();
        assert!(!list.is_empty());

        manager.cleanup(handle).await.unwrap();
    }

    #[tokio::test]
    async fn manager_detects_changes() {
        let dir = setup_test_repo();
        let config = WorktreeConfig::default();
        let manager = WorktreeManager::new(dir.path().to_path_buf(), config);

        let handle = manager
            .create_for_agent("dev", Some("change-test"), None)
            .await
            .unwrap();

        // Should be clean initially
        assert!(!manager.check_changes(&handle).await.unwrap());

        // Make a change
        std::fs::write(handle.path.join("new-file.txt"), "content").unwrap();
        assert!(manager.check_changes(&handle).await.unwrap());

        manager.cleanup(handle).await.unwrap();
    }

    #[tokio::test]
    async fn manager_get_or_create_reuses_existing() {
        let dir = setup_test_repo();
        let config = WorktreeConfig::default();
        let manager = WorktreeManager::new(dir.path().to_path_buf(), config);

        let h1 = manager
            .get_or_create("dev-auth", "dev", None)
            .await
            .unwrap();
        let h2 = manager
            .get_or_create("dev-auth", "dev", None)
            .await
            .unwrap();
        assert_eq!(h1.path, h2.path);

        manager.cleanup(h2).await.unwrap();
    }

    #[test]
    fn detect_path_conflicts_finds_overlap() {
        let dir = setup_test_repo();
        let config = WorktreeConfig {
            conflict_detection: ConflictDetection::FilePath,
            ..Default::default()
        };
        let manager = WorktreeManager::new(dir.path().to_path_buf(), config);

        let h1 = WorktreeHandle {
            repo_root: dir.path().to_path_buf(),
            path: dir.path().join("wt1"),
            branch: Some("b1".into()),
            has_changes: false,
            agent_name: "agent-a".into(),
            estimated_paths: vec!["src/auth/".into()],
            state: WorktreeState::Active,
        };
        let h2 = WorktreeHandle {
            repo_root: dir.path().to_path_buf(),
            path: dir.path().join("wt2"),
            branch: Some("b2".into()),
            has_changes: false,
            agent_name: "agent-b".into(),
            estimated_paths: vec!["src/auth/".into(), "src/api/".into()],
            state: WorktreeState::Active,
        };

        let conflicts = manager.detect_parallel_conflicts(&[&h1, &h2]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].other_agent, "agent-b");
    }
}
