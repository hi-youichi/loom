use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

pub struct WorkflowValidateSchemaTool {
    schema: Value,
    output_slot: Arc<Mutex<Option<Value>>>,
}

impl WorkflowValidateSchemaTool {
    pub fn new(schema: Value, output_slot: Arc<Mutex<Option<Value>>>) -> Self {
        Self {
            schema,
            output_slot,
        }
    }
}

#[async_trait]
impl Tool for WorkflowValidateSchemaTool {
    fn name(&self) -> &str {
        "workflow_validate_schema"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "workflow_validate_schema".to_string(),
            description: Some(
                "Submit your final structured result. \
                 You MUST call this tool to complete the task."
                    .to_string(),
            ),
            input_schema: self.schema.clone(),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        if let Err(msg) = validate_against_schema(&args, &self.schema) {
            tracing::warn!(
                target: "workflow::validate_schema",
                error = %msg,
                "schema validation failed",
            );
            return Ok(ToolCallContent::Text(format!(
                "Schema validation failed: {msg}. Please fix and retry."
            )));
        }

        tracing::debug!(
            target: "workflow::validate_schema",
            "schema validation passed, storing result",
        );
        *self.output_slot.lock().unwrap() = Some(args);
        Ok(ToolCallContent::Text("Result submitted.".to_string()))
    }
}

fn validate_against_schema(value: &Value, schema: &Value) -> Result<(), String> {
    if schema.as_object().is_none_or(|o| o.is_empty()) {
        return Ok(());
    }

    let validator = jsonschema::JSONSchema::compile(schema)
        .map_err(|e| format!("invalid schema definition: {e}"))?;

    let result = validator.validate(value);
    match result {
        Ok(()) => Ok(()),
        Err(errs) => {
            let messages: Vec<String> = errs.take(3).map(|e| format!("{e}")).collect();
            Err(messages.join("; "))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn structured_output_valid_json() {
        let slot = Arc::new(Mutex::new(None));
        let tool = WorkflowValidateSchemaTool::new(
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "count": {"type": "integer"}
                },
                "required": ["name"]
            }),
            slot.clone(),
        );

        let args = json!({"name": "test", "count": 42});
        let result = tool.call(args.clone(), None).await.unwrap();

        match result {
            ToolCallContent::Text(msg) => assert_eq!(msg, "Result submitted."),
            _ => panic!("expected Text"),
        }

        let stored = slot.lock().unwrap().clone();
        assert_eq!(stored, Some(args));
    }

    #[tokio::test]
    async fn structured_output_invalid_json() {
        let slot = Arc::new(Mutex::new(None));
        let tool = WorkflowValidateSchemaTool::new(
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"}
                },
                "required": ["name"]
            }),
            slot.clone(),
        );

        let args = json!({"count": 42});
        let result = tool.call(args, None).await.unwrap();

        match result {
            ToolCallContent::Text(msg) => {
                assert!(msg.starts_with("Schema validation failed"));
            }
            _ => panic!("expected Text"),
        }

        assert!(slot.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn structured_output_empty_schema_skips_validation() {
        let slot = Arc::new(Mutex::new(None));
        let tool = WorkflowValidateSchemaTool::new(json!({}), slot.clone());

        let args = json!({"anything": true});
        let result = tool.call(args.clone(), None).await.unwrap();

        match result {
            ToolCallContent::Text(msg) => assert_eq!(msg, "Result submitted."),
            _ => panic!("expected Text"),
        }
        assert_eq!(slot.lock().unwrap().clone(), Some(args));
    }

    #[test]
    fn structured_output_spec() {
        let tool = WorkflowValidateSchemaTool::new(json!({"type": "object"}), Arc::new(Mutex::new(None)));

assert_eq!(tool.name(), "workflow_validate_schema");
        let spec = tool.spec();
        assert_eq!(spec.name, "workflow_validate_schema");
        assert!(spec.description.as_ref().unwrap().contains("MUST"));
    }
}
