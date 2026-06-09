use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use task_core::{ShowError, TaskDb};

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;

pub use loom_types::tools::tool_name::TOOL_TASK_DELETE;

pub struct TaskDeleteTool {
    db: Arc<TaskDb>,
}

impl TaskDeleteTool {
    pub fn new(db: Arc<TaskDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Tool for TaskDeleteTool {
    fn name(&self) -> &str {
        TOOL_TASK_DELETE
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_TASK_DELETE.to_string(),
            description: Some("Delete a task by ID.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task ID or prefix" }
                },
                "required": ["id"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let id = args
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing 'id'".to_string()))?;

        let deleted = self.db.delete_task(id).await.map_err(|e| match e {
            ShowError::NotFound(_) => ToolSourceError::ToolError(e.to_string()),
            ShowError::Ambiguous { .. } => ToolSourceError::ToolError(e.to_string()),
            ShowError::DbError(_) => ToolSourceError::ToolError(e.to_string()),
        })?;

        let out = serde_json::to_string_pretty(&serde_json::json!({
            "id": deleted.id,
            "name": deleted.name,
            "deleted": true,
        }))
        .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
        Ok(ToolCallContent::text(out))
    }
}
