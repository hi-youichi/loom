use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use task_core::{ShowError, TaskDb};

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;

pub const TOOL_TASK_SHOW: &str = "task_show";

pub struct TaskShowTool {
    db: Arc<TaskDb>,
}

impl TaskShowTool {
    pub fn new(db: Arc<TaskDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Tool for TaskShowTool {
    fn name(&self) -> &str {
        TOOL_TASK_SHOW
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_TASK_SHOW.to_string(),
            description: Some("Show a task by ID (full UUID or prefix).".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task ID or prefix (>= 4 chars)" }
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

        let task = self.db.show_task(id).map_err(|e| match e {
            ShowError::NotFound(_) => ToolSourceError::ToolError(e.to_string()),
            ShowError::Ambiguous { .. } => ToolSourceError::ToolError(e.to_string()),
            ShowError::DbError(_) => ToolSourceError::ToolError(e.to_string()),
        })?;

        let out = serde_json::to_string_pretty(&task)
            .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
        Ok(ToolCallContent::text(out))
    }
}
