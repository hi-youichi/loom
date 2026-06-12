//! File tools registration: file operations under a working folder as tools.

use std::path::Path;
use std::sync::Arc;

use loom_skill::SkillUsageStore;
use loom_skill::SkillRegistry;
use tool_core::ToolRegistryLocked;

use crate::file::{
    ApplyPatchTool, CreateDirTool, DeleteFileTool, EditFileTool, GlobTool, GrepTool, LsTool,
    MoveFileTool, MultieditTool, ReadFileTool, WriteFileTool,
};
use crate::todo::{TodoReadTool, TodoWriteTool};
use crate::batch::BatchTool;
use crate::date::DateTool;

/// Registers file tools (ls, read, write_file, edit, move_file, delete_file, create_dir, glob, grep, todo_write, todo_read)
/// with the given registry.
///
/// The path must exist and be a directory; it is canonicalized before use.
///
/// # Errors
///
/// - [`tool_core::ToolSourceError::InvalidInput`] if the path does not exist, is not a directory,
///   or canonicalization fails.
///
/// # Interaction
///
/// Used by the ReAct builder when a working folder is set so file tools are
/// registered with the tool registry.
/// When `skill_registry` is `Some`, the skill tool uses the registry (discovery-based);
/// otherwise it uses the working folder's `.loom/skills` directory (legacy).
pub fn register_file_tools(
    registry: &ToolRegistryLocked,
    working_folder: impl AsRef<Path>,
    skill_registry: Option<Arc<SkillRegistry>>,
    skill_usage: Option<SkillUsageStore>,
) -> Result<(), tool_core::ToolSourceError> {
    let path = working_folder.as_ref();
    let canonical = path.canonicalize().map_err(|e| {
        tool_core::ToolSourceError::InvalidInput(format!(
            "failed to canonicalize working folder path: {}",
            e
        ))
    })?;

    if !canonical.is_dir() {
        return Err(tool_core::ToolSourceError::InvalidInput(format!(
            "working folder path is not a directory: {}",
            path.display()
        )));
    }

    let working_folder = Arc::new(canonical);

    // Register file tools
    registry.register_sync(Box::new(LsTool::new(working_folder.clone())));
    registry.register_sync(Box::new(ReadFileTool::new(working_folder.clone())));
    registry.register_sync(Box::new(WriteFileTool::new(working_folder.clone())));
    registry.register_sync(Box::new(EditFileTool::new(working_folder.clone())));
    registry.register_sync(Box::new(MoveFileTool::new(working_folder.clone())));
    registry.register_sync(Box::new(DeleteFileTool::new(working_folder.clone())));
    registry.register_sync(Box::new(CreateDirTool::new(working_folder.clone())));
    registry.register_sync(Box::new(GlobTool::new(working_folder.clone())));
    registry.register_sync(Box::new(GrepTool::new(working_folder.clone())));
    registry.register_sync(Box::new(MultieditTool::new(working_folder.clone())));
    registry.register_sync(Box::new(ApplyPatchTool::new(working_folder.clone())));

    // Register todo tools
    registry.register_sync(Box::new(TodoReadTool::new(working_folder.clone())));
    registry.register_sync(Box::new(TodoWriteTool::new(working_folder.clone())));

    // Note: Skill tool would need to be implemented separately
    // For now, we skip skill registration in the basic tools

    // Register other tools
    registry.register_sync(Box::new(BatchTool::new(working_folder.clone())));
    registry.register_sync(Box::new(DateTool::new(working_folder.clone())));

    Ok(())
}