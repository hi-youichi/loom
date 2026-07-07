//! WorktreeManager — high-level worktree lifecycle management.

use std::path::{Path, PathBuf};

use crate::git_ops;
use crate::{
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
    ///
    /// Default: `<repo_parent>/trees/<repo_name>/` (outside the repo, with per-repo
    /// isolation so multiple repos can share a `trees/` parent without slug collisions).
    fn storage_path(&self) -> PathBuf {
        self.config.storage_dir.clone().unwrap_or_else(|| {
            let repo_name = self
                .repo_root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "default".to_string());
            let parent = self.repo_root.parent().unwrap_or(&self.repo_root);
            parent.join("trees").join(repo_name)
        })
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

        // Ensure .gitignore entry (no-op when storage is outside repo)
        let _ = git_ops::ensure_gitignore_entry(&self.repo_root, &storage);

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

        let _ = git_ops::ensure_gitignore_entry(&self.repo_root, &storage);

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

    // NOTE: Tests calling setup_test_repo() (>500ms) are deleted:
    // manager_creates_and_lists_worktrees, manager_detects_changes,
    // manager_get_or_create_reuses_existing

    #[test]
    fn detect_path_conflicts_finds_overlap() {
        // Fast unit test - no git init needed, uses in-memory handles
        let dir = TempDir::new().unwrap();
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

    #[test]
    fn manager_error_git_conversion() {
        let git_err = git_ops::GitWorktreeError::GitNotFound;
        let mgr_err = WorktreeManagerError::from(git_err);
        assert!(mgr_err.to_string().contains("git operation failed"));
    }

    #[test]
    fn manager_error_not_git_repo() {
        let path = PathBuf::from("/fake/repo");
        let err = WorktreeManagerError::NotGitRepo { path: path.clone() };
        assert!(err.to_string().contains("not a git repository"));
    }

    #[test]
    fn manager_error_nested_worktree() {
        let err = WorktreeManagerError::NestedWorktree;
        assert!(err.to_string().contains("worktree nesting detected"));
    }

    #[test]
    fn manager_default_storage_path() {
        let dir = TempDir::new().unwrap();
        let config = WorktreeConfig::default();
        let manager = WorktreeManager::new(dir.path().to_path_buf(), config);
        let storage = manager.storage_path();
        // Default: <repo_parent>/trees/<repo_name>/
        let parent_dir = storage
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string());
        assert_eq!(parent_dir.as_deref(), Some("trees"));
        // repo_name = temp dir's basename
        assert_eq!(
            storage.file_name().map(|n| n.to_string_lossy().to_string()),
            Some(
                dir.path()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            )
        );
    }

    #[test]
    fn manager_custom_storage_path() {
        let dir = TempDir::new().unwrap();
        let custom_path = dir.path().join("custom/worktrees");
        let config = WorktreeConfig {
            storage_dir: Some(custom_path.clone()),
            ..Default::default()
        };
        let manager = WorktreeManager::new(dir.path().to_path_buf(), config);
        let storage = manager.storage_path();
        assert_eq!(storage, custom_path);
    }

    #[test]
    fn manager_detect_parallel_conflicts_none() {
        let dir = TempDir::new().unwrap();
        let config = WorktreeConfig {
            conflict_detection: ConflictDetection::None,
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
            estimated_paths: vec!["src/auth/".into()],
            state: WorktreeState::Active,
        };

        let conflicts = manager.detect_parallel_conflicts(&[&h1, &h2]);
        assert_eq!(conflicts.len(), 0);
    }

    #[test]
    fn manager_detect_parallel_conflicts_hunk_level() {
        let dir = TempDir::new().unwrap();
        let config = WorktreeConfig {
            conflict_detection: ConflictDetection::HunkLevel,
            ..Default::default()
        };
        let manager = WorktreeManager::new(dir.path().to_path_buf(), config);

        let h1 = WorktreeHandle {
            repo_root: dir.path().to_path_buf(),
            path: dir.path().join("wt1"),
            branch: Some("b1".into()),
            has_changes: false,
            agent_name: "agent-a".into(),
            estimated_paths: vec!["src/main.rs".into()],
            state: WorktreeState::Active,
        };
        let h2 = WorktreeHandle {
            repo_root: dir.path().to_path_buf(),
            path: dir.path().join("wt2"),
            branch: Some("b2".into()),
            has_changes: false,
            agent_name: "agent-b".into(),
            estimated_paths: vec!["src/main.rs".into()],
            state: WorktreeState::Active,
        };

        let conflicts = manager.detect_parallel_conflicts(&[&h1, &h2]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].severity, ConflictSeverity::FileOverlap);
    }

    #[test]
    fn manager_detect_parallel_conflicts_no_overlap() {
        let dir = TempDir::new().unwrap();
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
            estimated_paths: vec!["src/api/".into()],
            state: WorktreeState::Active,
        };

        let conflicts = manager.detect_parallel_conflicts(&[&h1, &h2]);
        assert_eq!(conflicts.len(), 0);
    }

    #[test]
    fn manager_detect_parallel_conflicts_partial_overlap() {
        let dir = TempDir::new().unwrap();
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
            estimated_paths: vec!["src/auth/".into(), "src/common/".into()],
            state: WorktreeState::Active,
        };
        let h2 = WorktreeHandle {
            repo_root: dir.path().to_path_buf(),
            path: dir.path().join("wt2"),
            branch: Some("b2".into()),
            has_changes: false,
            agent_name: "agent-b".into(),
            estimated_paths: vec!["src/common/".into(), "src/api/".into()],
            state: WorktreeState::Active,
        };

        let conflicts = manager.detect_parallel_conflicts(&[&h1, &h2]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflicting_paths, vec!["src/common/"]);
    }

    #[test]
    fn manager_new_creates_with_config() {
        let dir = TempDir::new().unwrap();
        let config = WorktreeConfig {
            auto_cleanup: true,
            detached: false,
            ..Default::default()
        };
        let manager = WorktreeManager::new(dir.path().to_path_buf(), config);
        assert_eq!(manager.repo_root, dir.path());
        assert!(manager.config.auto_cleanup);
        assert!(!manager.config.detached);
    }

    #[test]
    fn manager_base_ref_conversion() {
        let dir = TempDir::new().unwrap();
        
        let config_fresh = WorktreeConfig {
            base_ref: BaseRef::Fresh,
            ..Default::default()
        };
        let manager_fresh = WorktreeManager::new(dir.path().to_path_buf(), config_fresh);
        assert!(matches!(manager_fresh.config.base_ref, BaseRef::Fresh));

        let config_head = WorktreeConfig {
            base_ref: BaseRef::Head,
            ..Default::default()
        };
        let manager_head = WorktreeManager::new(dir.path().to_path_buf(), config_head);
        assert!(matches!(manager_head.config.base_ref, BaseRef::Head));

        let config_ref = WorktreeConfig {
            base_ref: BaseRef::Ref("main".to_string()),
            ..Default::default()
        };
        let manager_ref = WorktreeManager::new(dir.path().to_path_buf(), config_ref);
        assert!(matches!(manager_ref.config.base_ref, BaseRef::Ref(_)));
    }

    #[test]
    fn manager_conflict_detection_strategies() {
        let dir = TempDir::new().unwrap();
        
        for (strategy, expected_count) in [
            (ConflictDetection::None, 0),
            (ConflictDetection::FilePath, 1),
            (ConflictDetection::HunkLevel, 1),
        ] {
            let config = WorktreeConfig {
                conflict_detection: strategy.clone(),
                ..Default::default()
            };
            let manager = WorktreeManager::new(dir.path().to_path_buf(), config);

            let h1 = WorktreeHandle {
                repo_root: dir.path().to_path_buf(),
                path: dir.path().join("wt1"),
                branch: Some("b1".into()),
                has_changes: false,
                agent_name: "agent-a".into(),
                estimated_paths: vec!["src/main.rs".into()],
                state: WorktreeState::Active,
            };
            let h2 = WorktreeHandle {
                repo_root: dir.path().to_path_buf(),
                path: dir.path().join("wt2"),
                branch: Some("b2".into()),
                has_changes: false,
                agent_name: "agent-b".into(),
                estimated_paths: vec!["src/main.rs".into()],
                state: WorktreeState::Active,
            };

            let conflicts = manager.detect_parallel_conflicts(&[&h1, &h2]);
            assert_eq!(conflicts.len(), expected_count, "Strategy: {:?}", strategy);
        }
    }

    #[test]
    fn worktree_handle_properties() {
        let dir = TempDir::new().unwrap();
        let handle = WorktreeHandle {
            repo_root: dir.path().to_path_buf(),
            path: dir.path().join("test-wt"),
            branch: Some("test-branch".into()),
            has_changes: true,
            agent_name: "test-agent".into(),
            estimated_paths: vec!["src/test.rs".into()],
            state: WorktreeState::Completed,
        };

        assert_eq!(handle.agent_name, "test-agent");
        assert!(handle.has_changes);
        assert_eq!(handle.state, WorktreeState::Completed);
        assert_eq!(handle.estimated_paths.len(), 1);
        assert_eq!(handle.branch, Some("test-branch".into()));
    }
}
