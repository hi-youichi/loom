use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use task_core::{parse_status, CreateParams, TaskDb};

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;

pub const TOOL_TASK_CREATE: &str = "task_create";

pub struct TaskCreateTool {
    db: Arc<TaskDb>,
}

impl TaskCreateTool {
    pub fn new(db: Arc<TaskDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        TOOL_TASK_CREATE
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_TASK_CREATE.to_string(),
            description: Some("Create a new task.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name":        { "type": "string", "description": "Task name" },
                    "description": { "type": "string", "description": "Task description" },
                    "assignee":    { "type": "string", "description": "Assignee" },
                    "start_time":  { "type": "string", "description": "Start time (ISO 8601)" },
                    "status":      { "type": "string", "enum": ["pending","in_progress","completed","cancelled"], "description": "Task status" }
                },
                "required": ["name"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing 'name'".to_string()))?
            .to_string();

        let status_str = args
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending");
        let status = parse_status(status_str)
            .map_err(|e| ToolSourceError::InvalidInput(e))?;

        let params = CreateParams {
            name,
            description: args
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            assignee: args
                .get("assignee")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            start_time: args
                .get("start_time")
                .and_then(|v| v.as_str())
                .map(String::from),
            status,
        };

        let task = self.db.create_task(&params).await
            .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
        let out = serde_json::to_string_pretty(&task)
            .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
        Ok(ToolCallContent::text(out))
    }
}
