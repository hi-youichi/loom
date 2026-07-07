//! GitWorktreeTool: manage git worktrees for agent isolation.
//!
//! Provides direct access to worktree lifecycle without binding to an agent invocation:
//! - `list`:   show all active worktrees under `.loom/worktrees/`
//! - `create`: create a new worktree (optionally reuse existing)
//! - `cleanup`: remove a worktree and its branch
//! - `diff`:    show diff between worktree and base ref
//! - `changed`: list changed files in a worktree
//! - `prune`:   remove all trashed worktrees

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec, Tool};
use crate::agent::ReactBuildConfig;

use worktree::{WorktreeConfig, WorktreeManager};

pub const TOOL_GIT_WORKTREE: &str = "git_worktree";

pub struct GitWorktreeTool {
    #[allow(dead_code)]
    base_config: Arc<ReactBuildConfig>,
    repo_root: PathBuf,
}

impl GitWorktreeTool {
    pub fn new(base_config: Arc<ReactBuildConfig>) -> Self {
        let working_folder = base_config
            .working_folder
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new("."));
        // Always resolve to the actual git repo root so storage_path() can
        // derive a sane `<repo_parent>/trees/<repo_name>/` layout.
        // Using `working_folder` directly yields `.` or a relative path whose
        // `.file_name()` is None, falling back to `default`.
        let repo_root = worktree::git_ops::resolve_repo_root(working_folder)
            .unwrap_or_else(|_| working_folder.to_path_buf());
        Self { base_config, repo_root }
    }
}

#[async_trait]
impl Tool for GitWorktreeTool {
    fn name(&self) -> &str {
        TOOL_GIT_WORKTREE
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_GIT_WORKTREE.to_string(),
            description: Some(
                "Manage git worktrees for isolated parallel execution. \
                 Worktrees share the .git object DB (near-zero disk overhead) but have \
                 independent working directories and branches."
                    .into(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "enum": ["list", "create", "cleanup", "diff", "changed", "prune"],
                        "description": "The worktree operation to perform."
                    },
                    "slug": {
                        "type": "string",
                        "description": "Worktree identifier (used for create/cleanup/diff/changed)."
                    },
                    "base_ref": {
                        "type": "string",
                        "description": "Base ref for create (default: HEAD).",
                        "default": "HEAD"
                    },
                    "reuse": {
                        "type": "boolean",
                        "description": "If true and worktree exists, return existing instead of error (default: true).",
                        "default": true
                    },
                    "strategy": {
                        "type": "string",
                        "enum": ["sync", "async_trash"],
                        "description": "Cleanup strategy (default: sync).",
                        "default": "sync"
                    },
                    "auto_cleanup": {
                        "type": "boolean",
                        "description": "Auto-cleanup worktree with no changes on create (default: false).",
                        "default": false
                    }
                },
                "required": ["command"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing required: command".into()))?;

        let config = WorktreeConfig::default();
        let manager = WorktreeManager::new(self.repo_root.clone(), config);

        match command {
            "list" => cmd_list(&manager),
            "create" => cmd_create(&self.repo_root, &args).await,
            "cleanup" => cmd_cleanup(&self.repo_root, &args).await,
            "diff" => cmd_diff(&manager, &args).await,
            "changed" => cmd_changed(&manager, &args).await,
            "prune" => cmd_prune(&manager).await,
            _ => Err(ToolSourceError::InvalidInput(format!(
                "unknown command: {command}"
            ))),
        }
    }
}

fn cmd_list(manager: &WorktreeManager) -> Result<ToolCallContent, ToolSourceError> {
    let handles = manager
        .list_active()
        .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;

    let items: Vec<Value> = handles
        .iter()
        .map(|h| {
            json!({
                "slug": h.path.file_name().map(|n| n.to_string_lossy().to_string()),
                "path": h.path.display().to_string(),
                "branch": h.branch,
                "has_changes": h.has_changes,
                "state": format!("{:?}", h.state),
            })
        })
        .collect();

    Ok(ToolCallContent::text(
        serde_json::to_string_pretty(&json!({ "worktrees": items })).unwrap_or_default(),
    ))
}

async fn cmd_create(
    repo_root: &Path,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let slug = args
        .get("slug")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolSourceError::InvalidInput("missing required: slug".into()))?;

    let reuse = args
        .get("reuse")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if reuse {
        let config = parse_create_config(args);
        let manager = WorktreeManager::new(repo_root.to_path_buf(), config);
        let handle = manager
            .get_or_create(slug, slug, None)
            .await
            .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
        Ok(ToolCallContent::text(format!(
            "Worktree ready at: {}\nBranch: {}",
            handle.path.display(),
            handle.branch.as_deref().unwrap_or("(detached)")
        )))
    } else {
        let config = parse_create_config(args);
        let manager = WorktreeManager::new(repo_root.to_path_buf(), config);
        let handle = manager
            .create_for_agent(slug, None, None)
            .await
            .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
        Ok(ToolCallContent::text(format!(
            "Worktree created at: {}\nBranch: {}",
            handle.path.display(),
            handle.branch.as_deref().unwrap_or("(detached)")
        )))
    }
}

async fn cmd_cleanup(
    repo_root: &Path,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let slug = args
        .get("slug")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolSourceError::InvalidInput("missing required: slug".into()))?;

    let strategy = args
        .get("strategy")
        .and_then(|v| v.as_str())
        .unwrap_or("sync");

    let config = match strategy {
        "async_trash" => WorktreeConfig {
            cleanup_strategy: worktree::CleanupStrategy::AsyncTrash,
            ..Default::default()
        },
        _ => WorktreeConfig::default(),
    };

    let manager = WorktreeManager::new(repo_root.to_path_buf(), config);
    let handles = manager
        .list_active()
        .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;

    let target = handles.into_iter().find(|h| {
        h.path
            .file_name()
            .is_some_and(|n| n.to_string_lossy() == slug)
    });

    match target {
        Some(handle) => {
            let path_display = handle.path.display().to_string();
            manager
                .cleanup(handle)
                .await
                .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
            Ok(ToolCallContent::text(format!(
                "Cleaned up worktree: {path_display}"
            )))
        }
        None => Err(ToolSourceError::InvalidInput(format!(
            "worktree not found: {slug}"
        ))),
    }
}

async fn cmd_diff(
    manager: &WorktreeManager,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let slug = args
        .get("slug")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolSourceError::InvalidInput("missing required: slug".into()))?;

    let handles = manager
        .list_active()
        .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;

    let target = handles.into_iter().find(|h| {
        h.path
            .file_name()
            .is_some_and(|n| n.to_string_lossy() == slug)
    });

    match target {
        Some(handle) => {
            let diff = manager
                .diff_main(&handle)
                .await
                .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
            Ok(ToolCallContent::text(diff))
        }
        None => Err(ToolSourceError::InvalidInput(format!(
            "worktree not found: {slug}"
        ))),
    }
}

async fn cmd_changed(
    manager: &WorktreeManager,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let slug = args
        .get("slug")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolSourceError::InvalidInput("missing required: slug".into()))?;

    let handles = manager
        .list_active()
        .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;

    let target = handles.into_iter().find(|h| {
        h.path
            .file_name()
            .is_some_and(|n| n.to_string_lossy() == slug)
    });

    match target {
        Some(handle) => {
            let files = manager
                .changed_files(&handle)
                .await
                .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
            Ok(ToolCallContent::text(
                serde_json::to_string_pretty(&json!({ "files": files })).unwrap_or_default(),
            ))
        }
        None => Err(ToolSourceError::InvalidInput(format!(
            "worktree not found: {slug}"
        ))),
    }
}

async fn cmd_prune(manager: &WorktreeManager) -> Result<ToolCallContent, ToolSourceError> {
    let count = manager
        .prune_trash()
        .await
        .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
    Ok(ToolCallContent::text(format!(
        "Pruned {count} worktree(s) from trash"
    )))
}

fn parse_create_config(args: &Value) -> WorktreeConfig {
    let auto_cleanup = args
        .get("auto_cleanup")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    WorktreeConfig {
        auto_cleanup,
        ..Default::default()
    }
}
