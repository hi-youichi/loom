use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use task_core::{parse_status, TaskDb, UpdateParams};

use tool_core::{Tool, ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};

pub use tool_core::tool_name::TOOL_TASK_UPDATE;

pub struct TaskUpdateTool {
    db: Arc<TaskDb>,
}

impl TaskUpdateTool {
    pub fn new(db: Arc<TaskDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        TOOL_TASK_UPDATE
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_TASK_UPDATE.to_string(),
            description: Some("Update an existing task.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id":          { "type": "string", "description": "Task ID or prefix" },
                    "name":        { "type": "string", "description": "New name" },
                    "description": { "type": "string", "description": "New description" },
                    "assignee":    { "type": "string", "description": "New assignee" },
                    "start_time":  { "type": "string", "description": "New start time (ISO 8601)" },
                    "status":      { "type": "string", "enum": ["pending","in_progress","completed","cancelled"], "description": "New status" }
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
            .ok_or_else(|| ToolSourceError::InvalidInput("missing 'id'".to_string()))?
            .to_string();

        let status = args
            .get("status")
            .and_then(|v| v.as_str())
            .map(parse_status)
            .transpose()
            .map_err(|e: String| ToolSourceError::InvalidInput(e))?;

        let params = UpdateParams {
            id,
            name: args.get("name").and_then(|v| v.as_str()).map(String::from),
            description: args
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from),
            assignee: args
                .get("assignee")
                .and_then(|v| v.as_str())
                .map(String::from),
            start_time: args
                .get("start_time")
                .and_then(|v| v.as_str())
                .map(String::from),
            status,
        };

        let task = self
            .db
            .update_task(&params)
            .await
            .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
        let out = serde_json::to_string_pretty(&task)
            .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
        Ok(ToolCallContent::text(out))
    }
}
