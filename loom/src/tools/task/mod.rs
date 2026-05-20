mod create;
mod delete;
mod list;
mod show;
mod update;

pub use create::{TaskCreateTool, TOOL_TASK_CREATE};
pub use delete::{TaskDeleteTool, TOOL_TASK_DELETE};
pub use list::{TaskListTool, TOOL_TASK_LIST};
pub use show::{TaskShowTool, TOOL_TASK_SHOW};
pub use update::{TaskUpdateTool, TOOL_TASK_UPDATE};

use std::sync::Arc;

use crate::tools::AggregateToolSource;
use task_core::TaskDb;

pub async fn register_task_tools(aggregate: &AggregateToolSource, db: Arc<TaskDb>) {
    aggregate
        .register_async(Box::new(TaskCreateTool::new(db.clone())))
        .await;
    aggregate
        .register_async(Box::new(TaskShowTool::new(db.clone())))
        .await;
    aggregate
        .register_async(Box::new(TaskListTool::new(db.clone())))
        .await;
    aggregate
        .register_async(Box::new(TaskUpdateTool::new(db.clone())))
        .await;
    aggregate
        .register_async(Box::new(TaskDeleteTool::new(db)))
        .await;
}
