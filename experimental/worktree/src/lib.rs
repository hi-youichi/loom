//! Git worktree isolation for parallel sub-agent execution.
//!
//! This module provides file-system level isolation for sub-agents by creating
//! independent git worktrees. Each worktree has its own working directory,
//! branch, and file state while sharing the `.git` object database (near-zero
//! disk overhead).
//!
//! # Trigger paths (priority order)
//!
//! 1. `agent` tool parameter `isolation: "worktree"` — LLM specifies at call time
//! 2. Agent profile `config.yaml` field `isolation: worktree` — admin preset
//! 3. CLI flag `--worktree` / `-w` — user starts top-level session in worktree
//!
//! # Lifecycle
//!
//! ```text
//! pre-start → create → setup → use → evaluate → cleanup
//! ```

pub mod git_ops;
pub mod manager;

use std::path::{Path, PathBuf};

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

/// Detect if the current working directory is already inside a git worktree.
///
/// A regular clone has `.git` as a directory. A worktree has `.git` as a file
/// whose contents start with `gitdir:` pointing at the linked git metadata dir.
pub fn detect_worktree_nesting(working_dir: &Path) -> bool {
    let git_path = working_dir.join(".git");
    if git_path.is_file() {
        if let Ok(gitdir) = std::fs::read_to_string(&git_path) {
            return gitdir.trim_start().starts_with("gitdir:");
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

    #[test]
    fn base_ref_variants() {
        assert!(matches!(BaseRef::Fresh, BaseRef::Fresh));
        assert!(matches!(BaseRef::Head, BaseRef::Head));
        assert!(matches!(BaseRef::Ref("main".to_string()), BaseRef::Ref(_)));
    }

    #[test]
    fn base_ref_default() {
        let default_ref = BaseRef::default();
        assert!(matches!(default_ref, BaseRef::Fresh));
    }

    #[test]
    fn conflict_detection_variants() {
        assert!(matches!(ConflictDetection::None, ConflictDetection::None));
        assert!(matches!(ConflictDetection::FilePath, ConflictDetection::FilePath));
        assert!(matches!(ConflictDetection::HunkLevel, ConflictDetection::HunkLevel));
    }

    #[test]
    fn conflict_detection_default() {
        let default_cd = ConflictDetection::default();
        assert!(matches!(default_cd, ConflictDetection::None));
    }

    #[test]
    fn cleanup_strategy_variants() {
        assert!(matches!(CleanupStrategy::Sync, CleanupStrategy::Sync));
        assert!(matches!(CleanupStrategy::AsyncTrash, CleanupStrategy::AsyncTrash));
    }

    #[test]
    fn cleanup_strategy_default() {
        let default_cs = CleanupStrategy::default();
        assert!(matches!(default_cs, CleanupStrategy::Sync));
    }

    #[test]
    fn worktree_state_variants() {
        assert!(matches!(WorktreeState::Active, WorktreeState::Active));
        assert!(matches!(WorktreeState::Completed, WorktreeState::Completed));
        assert!(matches!(WorktreeState::Failed, WorktreeState::Failed));
        assert!(matches!(WorktreeState::Trashed, WorktreeState::Trashed));
    }

    #[test]
    fn worktree_state_equality() {
        assert_eq!(WorktreeState::Active, WorktreeState::Active);
        assert_ne!(WorktreeState::Active, WorktreeState::Completed);
    }

    #[test]
    fn conflict_severity_variants() {
        assert!(matches!(ConflictSeverity::FileOverlap, ConflictSeverity::FileOverlap));
        assert!(matches!(ConflictSeverity::HunkOverlap, ConflictSeverity::HunkOverlap));
    }

    #[test]
    fn conflict_severity_equality() {
        assert_eq!(ConflictSeverity::FileOverlap, ConflictSeverity::FileOverlap);
        assert_ne!(ConflictSeverity::FileOverlap, ConflictSeverity::HunkOverlap);
    }

    #[test]
    fn worktree_config_default() {
        let config = WorktreeConfig::default();
        assert!(matches!(config.base_ref, BaseRef::Fresh));
        assert!(config.storage_dir.is_none());
        assert!(config.branch_prefix.is_none());
        assert!(!config.auto_cleanup);
        assert!(!config.detached);
        assert!(config.include_patterns.is_empty());
        assert!(config.symlink_patterns.is_empty());
        assert!(config.shared_cache_dirs.is_empty());
        assert!(config.sparse_paths.is_empty());
        assert!(matches!(config.conflict_detection, ConflictDetection::None));
        assert!(matches!(config.cleanup_strategy, CleanupStrategy::Sync));
    }

    #[test]
    fn worktree_config_clone() {
        let config = WorktreeConfig {
            auto_cleanup: true,
            branch_prefix: Some("custom-".to_string()),
            ..Default::default()
        };

        let cloned = config.clone();
        assert!(cloned.auto_cleanup);
        assert_eq!(cloned.branch_prefix, Some("custom-".to_string()));
    }

    #[test]
    fn worktree_config_with_all_fields() {
        let config = WorktreeConfig {
            base_ref: BaseRef::Ref("main".to_string()),
            storage_dir: Some(PathBuf::from("/custom/path")),
            branch_prefix: Some("test-".to_string()),
            auto_cleanup: true,
            detached: false,
            include_patterns: vec!["*.txt".to_string()],
            symlink_patterns: vec!["node_modules".to_string()],
            shared_cache_dirs: vec!["target/".to_string()],
            sparse_paths: vec!["src/".to_string()],
            conflict_detection: ConflictDetection::FilePath,
            cleanup_strategy: CleanupStrategy::AsyncTrash,
        };

        assert!(matches!(config.base_ref, BaseRef::Ref(_)));
        assert_eq!(config.storage_dir, Some(PathBuf::from("/custom/path")));
        assert_eq!(config.branch_prefix, Some("test-".to_string()));
        assert!(config.auto_cleanup);
        assert!(!config.detached);
        assert_eq!(config.include_patterns.len(), 1);
        assert_eq!(config.symlink_patterns.len(), 1);
        assert_eq!(config.shared_cache_dirs.len(), 1);
        assert_eq!(config.sparse_paths.len(), 1);
        assert!(matches!(config.conflict_detection, ConflictDetection::FilePath));
        assert!(matches!(config.cleanup_strategy, CleanupStrategy::AsyncTrash));
    }

    #[test]
    fn sanitize_slug_empty_string() {
        assert_eq!(sanitize_slug(""), "");
    }

    #[test]
    fn sanitize_unicode_chars() {
        assert_eq!(sanitize_slug("用户任务"), "用户任务");
    }

    #[test]
    fn sanitize_slug_special_only() {
        assert_eq!(sanitize_slug("@#$%"), "");
    }

    #[test]
    fn sanitize_slug_preserves_case() {
        assert_eq!(sanitize_slug("MyTask"), "MyTask");
    }

    #[test]
    fn sanitize_slug_numbers() {
        assert_eq!(sanitize_slug("task123"), "task123");
    }

    #[test]
    fn worktree_profile_config_all_fields() {
        let yaml = r#"
base_ref: "origin/main"
auto_cleanup: true
cleanup_strategy: "sync"
conflict_detection: "hunk_level"
include:
  - "*.env"
  - "*.md"
symlink:
  - "node_modules"
shared_cache:
  - "target/"
  - ".next/"
sparse_paths:
  - "src/"
  - "tests/"
"#;
        let profile: WorktreeProfileConfig = serde_yaml::from_str(yaml).unwrap();
        
        assert_eq!(profile.base_ref, Some("origin/main".to_string()));
        assert_eq!(profile.auto_cleanup, Some(true));
        assert_eq!(profile.cleanup_strategy, Some("sync".to_string()));
        assert_eq!(profile.conflict_detection, Some("hunk_level".to_string()));
        assert_eq!(profile.include.len(), 2);
        assert_eq!(profile.symlink.len(), 1);
        assert_eq!(profile.shared_cache.len(), 2);
        assert_eq!(profile.sparse_paths.len(), 2);
    }

    #[test]
    fn worktree_profile_config_to_config_conversions() {
        let test_cases = vec![
            ("base_ref: fresh", BaseRef::Fresh),
            ("base_ref: head", BaseRef::Head),
            ("base_ref: \"custom\"", BaseRef::Ref("custom".to_string())),
        ];

        for (yaml, _expected_base_ref) in test_cases {
            let profile: WorktreeProfileConfig = serde_yaml::from_str(yaml).unwrap();
            let config = profile.to_worktree_config();
            assert!(matches!(config.base_ref, _expected_base_ref));
        }
    }

    #[test]
    fn worktree_profile_config_cleanup_strategies() {
        let test_cases = vec![
            ("cleanup_strategy: sync", CleanupStrategy::Sync),
            ("cleanup_strategy: async_trash", CleanupStrategy::AsyncTrash),
            ("cleanup_strategy: invalid", CleanupStrategy::Sync), // defaults to sync
        ];

        for (yaml, _expected_strategy) in test_cases {
            let profile: WorktreeProfileConfig = serde_yaml::from_str(yaml).unwrap();
            let config = profile.to_worktree_config();
            assert!(matches!(config.cleanup_strategy, _expected_strategy));
        }
    }

    #[test]
    fn worktree_profile_config_conflict_detection() {
        let test_cases = vec![
            ("conflict_detection: none", ConflictDetection::None),
            ("conflict_detection: file_path", ConflictDetection::FilePath),
            ("conflict_detection: hunk_level", ConflictDetection::HunkLevel),
            ("conflict_detection: invalid", ConflictDetection::None), // defaults to none
        ];

        for (yaml, _expected_cd) in test_cases {
            let profile: WorktreeProfileConfig = serde_yaml::from_str(yaml).unwrap();
            let config = profile.to_worktree_config();
            assert!(matches!(config.conflict_detection, _expected_cd));
        }
    }

    #[test]
    fn worktree_profile_config_auto_cleanup_defaults() {
        let profile: WorktreeProfileConfig = serde_yaml::from_str("{}").unwrap();
        let config = profile.to_worktree_config();
        assert!(config.auto_cleanup); // defaults to true
    }

    #[test]
    fn conflict_info_structure() {
        let info = ConflictInfo {
            other_agent: "agent-b".to_string(),
            conflicting_paths: vec!["src/main.rs".to_string(), "src/utils.rs".to_string()],
            severity: ConflictSeverity::FileOverlap,
        };

        assert_eq!(info.other_agent, "agent-b");
        assert_eq!(info.conflicting_paths.len(), 2);
        assert!(matches!(info.severity, ConflictSeverity::FileOverlap));
    }

    #[test]
    fn detect_worktree_nesting_with_git_file() {
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();
        let git_dir = dir.path().join(".git");
        
        // Create a .git file that points to a Loom worktree
        std::fs::write(&git_dir, "gitdir: /path/to/.loom/worktrees/agent-123/.git").unwrap();
        
        assert!(detect_worktree_nesting(dir.path()));
    }

#[test]
    fn detect_worktree_nesting_with_non_loom_git_file() {
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();
        let git_dir = dir.path().join(".git");

        // Any `.git` file (not directory) with `gitdir:` contents signals
        // we are inside SOME worktree, even if not Loom-managed.
        std::fs::write(&git_dir, "gitdir: /other/path/.git").unwrap();

        assert!(detect_worktree_nesting(dir.path()));
    }

    #[test]
    fn detect_worktree_nesting_without_git_file() {
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();
        
        // No .git file exists
        assert!(!detect_worktree_nesting(dir.path()));
    }

    #[test]
    fn worktree_state_debug() {
        // Verify WorktreeState implements Debug
        let state = WorktreeState::Active;
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("Active"));
    }

    #[test]
    fn conflict_severity_debug() {
        // Verify ConflictSeverity implements Debug
        let severity = ConflictSeverity::FileOverlap;
        let debug_str = format!("{:?}", severity);
        assert!(debug_str.contains("FileOverlap"));
    }

    #[test]
    fn sanitize_slug_preserves_underscores() {
        assert_eq!(sanitize_slug("task_name"), "task_name");
    }

    #[test]
    fn sanitize_slug_mixed_special_chars() {
        assert_eq!(sanitize_slug("task-name_v2.test"), "task-name_v2test");
    }

    #[test]
    fn worktree_profile_config_json_parsing() {
        let json = r#"{
            "base_ref": "main",
            "auto_cleanup": false,
            "cleanup_strategy": "async_trash",
            "conflict_detection": "file_path"
        }"#;
        
        let profile: WorktreeProfileConfig = serde_json::from_str(json).unwrap();
        let config = profile.to_worktree_config();
        
        assert!(matches!(config.base_ref, BaseRef::Ref(_)));
        assert!(!config.auto_cleanup);
        assert!(matches!(config.cleanup_strategy, CleanupStrategy::AsyncTrash));
        assert!(matches!(config.conflict_detection, ConflictDetection::FilePath));
    }
}