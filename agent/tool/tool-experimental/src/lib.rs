pub mod file_memory;
pub mod memory;
pub mod conversation;
pub mod task;
mod help;

pub use file_memory::{MemoryTool, TOOL_MEMORY};
pub use memory::*;
pub use conversation::*;
pub use task::*;
pub use help::{HelpTool, TOOL_HELP};

use std::sync::Arc;
use tool_core::ToolRegistryLocked;
use loom_memory::{Namespace, Store};
use memory_v2::MemoryStore;
use task_core::TaskDb;

pub async fn register_file_memory_tool(
    registry: &ToolRegistryLocked,
    store: Arc<MemoryStore>,
) {
    registry
        .register_async(Box::new(MemoryTool::new(store)))
        .await;
}

#[allow(dead_code)]
pub async fn register_memory_tools(registry: &ToolRegistryLocked, store: Arc<dyn Store>, namespace: Namespace) {
    registry.register_async(Box::new(memory::RememberTool::new(store.clone(), namespace.clone()))).await;
    registry.register_async(Box::new(memory::RecallTool::new(store.clone(), namespace.clone()))).await;
    registry.register_async(Box::new(memory::SearchMemoriesTool::new(store.clone(), namespace.clone()))).await;
    registry.register_async(Box::new(memory::ListMemoriesTool::new(store, namespace))).await;
    registry.register_async(Box::new(conversation::GetRecentMessagesTool::new())).await;
}

pub async fn register_task_tools(registry: &ToolRegistryLocked, db: Arc<TaskDb>) {
    registry.register_async(Box::new(task::TaskCreateTool::new(db.clone()))).await;
    registry.register_async(Box::new(task::TaskShowTool::new(db.clone()))).await;
    registry.register_async(Box::new(task::TaskListTool::new(db.clone()))).await;
    registry.register_async(Box::new(task::TaskUpdateTool::new(db.clone()))).await;
    registry.register_async(Box::new(task::TaskDeleteTool::new(db))).await;
}