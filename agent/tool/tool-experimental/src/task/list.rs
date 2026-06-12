use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use task_core::{parse_status, ListParams, TaskDb};

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec, Tool};

pub use loom_types::tools::tool_name::TOOL_TASK_LIST;

pub struct TaskListTool {
    db: Arc<TaskDb>,
}

impl TaskListTool {
    pub fn new(db: Arc<TaskDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &str {
        TOOL_TASK_LIST
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_TASK_LIST.to_string(),
            description: Some("List tasks with optional filters and pagination.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status":      { "type": "string", "enum": ["pending","in_progress","completed","cancelled"], "description": "Filter by status" },
                    "assignee":    { "type": "string", "description": "Filter by assignee" },
                    "name":        { "type": "string", "description": "Filter by name (substring match)" },
                    "sort_by":     { "type": "string", "enum": ["created_at","start_time","name","status"], "description": "Sort field" },
                    "sort_order":  { "type": "string", "enum": ["asc","desc"], "description": "Sort order" },
                    "limit":       { "type": "integer", "description": "Page size" },
                    "page":        { "type": "integer", "description": "Page number (1-based)" }
                }
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let status = args
            .get("status")
            .and_then(|v| v.as_str())
            .map(parse_status)
            .transpose()
            .map_err(|e: String| ToolSourceError::InvalidInput(e))?;

        let params = ListParams {
            status,
            assignee: args.get("assignee").and_then(|v| v.as_str()).map(String::from),
            name: args.get("name").and_then(|v| v.as_str()).map(String::from),
            sort_by: args
                .get("sort_by")
                .and_then(|v| v.as_str())
                .unwrap_or("created_at")
                .to_string(),
            sort_order: args
                .get("sort_order")
                .and_then(|v| v.as_str())
                .unwrap_or("desc")
                .to_string(),
            limit: args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20) as u32,
            page: args
                .get("page")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32,
        };

        let list = self.db.list_tasks(&params).await
            .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
        let out = serde_json::to_string_pretty(&list)
            .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
        Ok(ToolCallContent::text(out))
    }
}
