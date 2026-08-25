pub mod conversation;
pub mod file_memory;
mod help;
pub mod llm;
pub mod memory;
pub mod task;

pub use conversation::*;
pub use file_memory::{MemoryTool, TOOL_MEMORY};
pub use help::{HelpTool, TOOL_HELP};
pub use llm::{LlmProviderData, LlmTool, LlmToolConfig, LlmToolData};
pub use memory::*;
pub use task::*;

use memory_v2::MemoryStore;
use std::sync::Arc;
use task_core::TaskDb;
use tool_core::ToolRegistryLocked;

pub async fn register_file_memory_tool(registry: &ToolRegistryLocked, store: Arc<MemoryStore>) {
    registry
        .register_async(Box::new(MemoryTool::new(store)))
        .await;
}

/// Register a foreground `MemoryTool` honouring the `user_profile_enabled` guard.
///
/// When `memory_enabled=false` AND `user_profile_enabled=false` this is a no-op
/// (caller should check before calling — see `tool_source.rs`).
/// When `user_profile_enabled=false`, writes targeting `USER.md` are rejected at runtime.
pub async fn register_file_memory_tool_guarded(
    registry: &ToolRegistryLocked,
    store: Arc<MemoryStore>,
    user_profile_enabled: bool,
) {
    registry
        .register_async(Box::new(MemoryTool::for_foreground(
            store,
            user_profile_enabled,
        )))
        .await;
}

pub async fn register_task_tools(registry: &ToolRegistryLocked, db: Arc<TaskDb>) {
    registry
        .register_async(Box::new(task::TaskCreateTool::new(db.clone())))
        .await;
    registry
        .register_async(Box::new(task::TaskShowTool::new(db.clone())))
        .await;
    registry
        .register_async(Box::new(task::TaskListTool::new(db.clone())))
        .await;
    registry
        .register_async(Box::new(task::TaskUpdateTool::new(db.clone())))
        .await;
    registry
        .register_async(Box::new(task::TaskDeleteTool::new(db)))
        .await;
}
