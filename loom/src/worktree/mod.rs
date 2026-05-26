//! Git worktree isolation for parallel sub-agent execution.
//!
//! This module provides file-system level isolation for sub-agents by creating
//! independent git worktrees. Each worktree has its own working directory,
//! branch, and file state while sharing the `.git` object database (near-zero
//! disk overhead).
//!
//! # Trigger paths (priority order)
//!
//! 1. `invoke_agent` parameter `isolation: "worktree"` — LLM specifies at call time
//! 2. Agent profile `config.yaml` field `isolation: worktree` — admin preset
//! 3. CLI flag `--worktree` / `-w` — user starts top-level session in worktree
//!
//! # Lifecycle
//!
//! ```text
//! pre-start → create → setup → use → evaluate → cleanup
//! ```

mod git_ops;
mod manager;

use std::path::PathBuf;

pub use git_ops::GitWorktreeError;
pub use manager::WorktreeManager;

/// Base reference for new worktree creation.
#[derive(Clone, Debug, Default)]
pub enum BaseRef {
    /// Create from HEAD with a clean working tree (safest default).
    #[default]
    Fresh,
    /// Create from HEAD, keeping the branch name (not detached).
    Head,
    /// Create from a specific ref (e.g. `origin/main`, `v1.0.0`).
    Ref(String),
}

/// Pre-merge conflict detection strategy.
#[derive(Clone, Debug, Default)]
pub enum ConflictDetection {
    /// No detection.
    #[default]
    None,
    /// Compare file paths modified by each worktree.
    FilePath,
    /// Compare specific diff hunk ranges (more precise but slower).
    HunkLevel,
}

/// Cleanup strategy for worktree removal.
#[derive(Clone, Debug, Default)]
pub enum CleanupStrategy {
    /// Synchronous removal (blocks until complete).
    #[default]
    Sync,
    /// Move to `.trash/` first, then prune in background.
    AsyncTrash,
}

/// Worktree lifecycle state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreeState {
    /// Worktree is in use by an agent.
    Active,
    /// Agent has finished; worktree preserved for review.
    Completed,
    /// Agent or worktree operation failed.
    Failed,
    /// Moved to trash directory, pending prune.
    Trashed,
}

/// Configuration for worktree creation.
#[derive(Clone, Debug, Default)]
pub struct WorktreeConfig {
    /// Base ref for the new worktree branch.
    pub base_ref: BaseRef,
    /// Override storage directory (default: `.loom/worktrees/`).
    pub storage_dir: Option<PathBuf>,
    /// Prefix for auto-generated branch names (default: `worktree-`).
    pub branch_prefix: Option<String>,
    /// Auto-cleanup worktrees with no changes on agent completion.
    pub auto_cleanup: bool,
    /// Create a detached HEAD worktree (no branch created).
    pub detached: bool,
    /// File patterns to copy from main checkout (`.worktreeinclude`).
    pub include_patterns: Vec<String>,
    /// File patterns to symlink instead of copy (large files).
    pub symlink_patterns: Vec<String>,
    /// Build cache directories to share across worktrees (e.g. `target/`, `node_modules/`).
    pub shared_cache_dirs: Vec<String>,
    /// Sparse checkout paths (monorepo optimization).
    pub sparse_paths: Vec<String>,
    /// Pre-merge conflict detection strategy.
    pub conflict_detection: ConflictDetection,
    /// Cleanup strategy.
    pub cleanup_strategy: CleanupStrategy,
}

/// Runtime handle to an active worktree.
#[derive(Debug)]
pub struct WorktreeHandle {
    /// Root of the main git repository.
    pub repo_root: PathBuf,
    /// Absolute path to the worktree working directory.
    pub path: PathBuf,
    /// Branch name (None if detached HEAD).
    pub branch: Option<String>,
    /// Whether the worktree has uncommitted changes.
    pub has_changes: bool,
    /// Name of the agent using this worktree.
    pub agent_name: String,
    /// File paths the task is expected to modify (for conflict detection).
    pub estimated_paths: Vec<String>,
    /// Current lifecycle state.
    pub state: WorktreeState,
}

/// Simplified worktree config that can be parsed from agent profile YAML.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct WorktreeProfileConfig {
    pub base_ref: Option<String>,
    pub auto_cleanup: Option<bool>,
    pub cleanup_strategy: Option<String>,
    pub conflict_detection: Option<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub symlink: Vec<String>,
    #[serde(default)]
    pub shared_cache: Vec<String>,
    #[serde(default)]
    pub sparse_paths: Vec<String>,
}

impl WorktreeProfileConfig {
    /// Convert profile config to a `WorktreeConfig`, resolving string values.
    pub fn to_worktree_config(&self) -> WorktreeConfig {
        WorktreeConfig {
            base_ref: match self.base_ref.as_deref() {
                Some("head") => BaseRef::Head,
                Some(r) => BaseRef::Ref(r.to_string()),
                None => BaseRef::Fresh,
            },
            auto_cleanup: self.auto_cleanup.unwrap_or(true),
            cleanup_strategy: match self.cleanup_strategy.as_deref() {
                Some("async_trash") => CleanupStrategy::AsyncTrash,
                _ => CleanupStrategy::Sync,
            },
            conflict_detection: match self.conflict_detection.as_deref() {
                Some("file_path") => ConflictDetection::FilePath,
                Some("hunk_level") => ConflictDetection::HunkLevel,
                _ => ConflictDetection::None,
            },
            include_patterns: self.include.clone(),
            symlink_patterns: self.symlink.clone(),
            shared_cache_dirs: self.shared_cache.clone(),
            sparse_paths: self.sparse_paths.clone(),
            ..Default::default()
        }
    }
}

/// Conflict information between worktrees.
#[derive(Debug)]
pub struct ConflictInfo {
    /// Name of the other agent's worktree.
    pub other_agent: String,
    /// File paths that overlap.
    pub conflicting_paths: Vec<String>,
    /// Severity of the conflict.
    pub severity: ConflictSeverity,
}

/// Conflict severity level.
#[derive(Debug, PartialEq, Eq)]
pub enum ConflictSeverity {
    /// Same file touched — tasks should be serialized.
    FileOverlap,
    /// Same file, same region — very likely to cause merge conflicts.
    HunkOverlap,
}

/// Sanitize a name into a filesystem-safe slug (max 32 chars).
pub fn sanitize_slug(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect()
}

/// Detect if the current working directory is already inside a Loom-managed worktree.
pub fn detect_worktree_nesting(working_dir: &PathBuf) -> bool {
    let git_path = working_dir.join(".git");
    if git_path.exists() && git_path.is_file() {
        if let Ok(gitdir) = std::fs::read_to_string(&git_path) {
            return gitdir.contains(".loom/worktrees");
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_slug_filters_special_chars() {
        assert_eq!(sanitize_slug("task/auth feature"), "taskauthfeature");
    }

    #[test]
    fn sanitize_slug_truncates_long_names() {
        let long = "a".repeat(100);
        assert_eq!(sanitize_slug(&long).len(), 32);
    }

    #[test]
    fn sanitize_slug_keeps_alphanumeric_and_hyphens() {
        assert_eq!(sanitize_slug("my-task_v2"), "my-task_v2");
    }

    #[test]
    fn profile_config_defaults() {
        let cfg: WorktreeProfileConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.base_ref.is_none());
        assert!(cfg.auto_cleanup.is_none());
        assert!(cfg.include.is_empty());
    }

    #[test]
    fn profile_config_to_worktree_config() {
        let yaml = r#"
base_ref: head
auto_cleanup: false
cleanup_strategy: async_trash
conflict_detection: file_path
include:
  - ".env"
shared_cache:
  - "target/"
"#;
        let profile: WorktreeProfileConfig = serde_yaml::from_str(yaml).unwrap();
        let cfg = profile.to_worktree_config();
        assert!(matches!(cfg.base_ref, BaseRef::Head));
        assert!(!cfg.auto_cleanup);
        assert!(matches!(cfg.cleanup_strategy, CleanupStrategy::AsyncTrash));
        assert!(matches!(cfg.conflict_detection, ConflictDetection::FilePath));
        assert_eq!(cfg.include_patterns, vec![".env"]);
        assert_eq!(cfg.shared_cache_dirs, vec!["target/"]);
    }
}
