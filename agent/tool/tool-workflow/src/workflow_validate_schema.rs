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
    submit_notify: Arc<tokio::sync::Notify>,
}

impl WorkflowValidateSchemaTool {
    pub fn new(
        schema: Value,
        output_slot: Arc<Mutex<Option<Value>>>,
        submit_notify: Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            schema,
            output_slot,
            submit_notify,
        }
    }
}

#[async_trait]
impl Tool for WorkflowValidateSchemaTool {
    fn name(&self) -> &str {
        "workflow_validate_schema"
    }

    fn spec(&self) -> ToolSpec {
        let input_schema = wrap_in_result_envelope(&self.schema);
        ToolSpec {
            name: "workflow_validate_schema".to_string(),
            description: Some(
                "Submit your final structured result. \
                 You MUST call this tool to complete the task. \
                 Pass your result as {\"result\": <your JSON value>}."
                    .to_string(),
            ),
            input_schema,
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let inner = extract_result(&args);

        if let Err(msg) = validate_against_schema(inner, &self.schema) {
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
        *self.output_slot.lock().unwrap() = Some(inner.clone());
        self.submit_notify.notify_one();
        Ok(ToolCallContent::Text("Result submitted.".to_string()))
    }
}

fn wrap_in_result_envelope(schema: &Value) -> Value {
    if schema.as_object().is_none_or(|o| o.is_empty()) {
        return serde_json::json!({
            "type": "object",
            "properties": {
                "result": {}
            },
            "required": ["result"]
        });
    }
    serde_json::json!({
        "type": "object",
        "properties": {
            "result": schema
        },
        "required": ["result"]
    })
}

fn extract_result(args: &Value) -> &Value {
    args.get("result").unwrap_or(args)
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

    fn make_tool(schema: Value) -> (WorkflowValidateSchemaTool, Arc<Mutex<Option<Value>>>) {
        let slot = Arc::new(Mutex::new(None));
        let notify = Arc::new(tokio::sync::Notify::new());
        let tool = WorkflowValidateSchemaTool::new(schema, slot.clone(), notify);
        (tool, slot)
    }

    #[tokio::test]
    async fn structured_output_valid_json() {
        let (tool, slot) = make_tool(json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "count": {"type": "integer"}
            },
            "required": ["name"]
        }));

        let payload = json!({"name": "test", "count": 42});
        let args = json!({"result": payload});
        let result = tool.call(args, None).await.unwrap();

        match result {
            ToolCallContent::Text(msg) => assert_eq!(msg, "Result submitted."),
            _ => panic!("expected Text"),
        }

        let stored = slot.lock().unwrap().clone();
        assert_eq!(stored, Some(payload));
    }

    #[tokio::test]
    async fn structured_output_invalid_json() {
        let (tool, slot) = make_tool(json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            },
            "required": ["name"]
        }));

        let args = json!({"result": {"count": 42}});
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
        let (tool, slot) = make_tool(json!({}));

        let payload = json!({"anything": true});
        let args = json!({"result": payload});
        let result = tool.call(args, None).await.unwrap();

        match result {
            ToolCallContent::Text(msg) => assert_eq!(msg, "Result submitted."),
            _ => panic!("expected Text"),
        }
        assert_eq!(slot.lock().unwrap().clone(), Some(payload));
    }

    #[test]
    fn structured_output_spec() {
        let (tool, _) = make_tool(json!({"type": "object"}));

        assert_eq!(tool.name(), "workflow_validate_schema");
        let spec = tool.spec();
        assert_eq!(spec.name, "workflow_validate_schema");
        assert!(spec.description.as_ref().unwrap().contains("MUST"));

        let schema = &spec.input_schema;
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["result"]["type"], "object");
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("result")));
    }
}
