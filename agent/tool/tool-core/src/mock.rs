use async_trait::async_trait;
use serde_json::{json, Value};

use crate::{Tool, ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};

pub struct MockTool {
    name: String,
    spec: ToolSpec,
    call_result: String,
}

impl MockTool {
    pub fn get_time_example() -> Box<Self> {
        Box::new(Self {
            name: "get_time".to_string(),
            spec: ToolSpec {
                name: "get_time".to_string(),
                description: Some("Get current time.".to_string()),
                input_schema: json!({ "type": "object", "properties": {} }),
                output_hint: None,
            },
            call_result: "2025-01-29 12:00:00".to_string(),
        })
    }

    pub fn new(name: &str, description: &str, call_result: String) -> Box<Self> {
        Box::new(Self {
            name: name.to_string(),
            spec: ToolSpec {
                name: name.to_string(),
                description: Some(description.to_string()),
                input_schema: json!({ "type": "object", "properties": {} }),
                output_hint: None,
            },
            call_result,
        })
    }

    pub fn with_call_result(mut self: Box<Self>, text: String) -> Box<Self> {
        self.call_result = text;
        self
    }
}

pub fn mock_registry() -> std::sync::Arc<crate::ToolRegistryLocked> {
    let registry = crate::ToolRegistryLocked::new();
    let tool = MockTool::get_time_example();
    registry.register_sync(tool);
    std::sync::Arc::new(registry)
}

#[async_trait]
impl Tool for MockTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(
        &self,
        _args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        Ok(ToolCallContent::text(self.call_result.clone()))
    }
}
