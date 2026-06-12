//! Batch tool: run multiple independent tool calls in parallel (1–25 per batch).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError, Tool};

pub use loom_types::tools::tool_name::TOOL_BATCH;

const MAX_CALLS: usize = 25;

/// Tool that executes multiple tool calls in parallel.
pub struct BatchTool {
    #[allow(dead_code)]
    working_folder: Arc<std::path::PathBuf>,
}

impl BatchTool {
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for BatchTool {
    fn name(&self) -> &str {
        TOOL_BATCH
    }

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
            name: TOOL_BATCH.to_string(),
            description: Some(
                "Execute multiple independent tool calls in parallel (1–25 per batch). \
                 Payload: JSON array of { \"tool\", \"parameters\" }. Do not nest batch inside batch."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "calls": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "tool": { "type": "string", "description": "Tool name (e.g. read, grep, bash/powershell)." },
                                "parameters": { "type": "object", "description": "Arguments for the tool." }
                            },
                            "required": ["tool", "parameters"]
                        },
                        "minItems": 1,
                        "maxItems": 25,
                        "description": "List of tool calls to run in parallel."
                    }
                },
                "required": ["calls"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let calls = args
            .get("calls")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ToolSourceError::InvalidInput("missing or invalid 'calls' array".to_string())
            })?;

        if calls.is_empty() || calls.len() > MAX_CALLS {
            return Err(ToolSourceError::InvalidInput(format!(
                "calls must have 1–{} items, got {}",
                MAX_CALLS,
                calls.len()
            )));
        }

        // For now, return a placeholder message
        // The full implementation would need access to the tool registry
        Ok(ToolCallContent::text(format!(
            "Batch tool called with {} calls. Full implementation requires tool registry integration.",
            calls.len()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_batch() -> BatchTool {
        let temp_dir = tempfile::tempdir().unwrap();
        BatchTool::new(Arc::new(temp_dir.path().to_path_buf()))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_calls_array() {
        let batch = make_batch();
        let err = batch.call(json!({}), None).await.unwrap_err();
        assert!(err.to_string().contains("missing or invalid 'calls' array"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn empty_calls_array() {
        let batch = make_batch();
        let err = batch.call(json!({"calls": []}), None).await.unwrap_err();
        assert!(err.to_string().contains("calls must have 1–25 items"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn basic_call() {
        let batch = make_batch();
        let result = batch
            .call(
                json!({"calls": [{"tool": "echo_a", "parameters": {}}]}),
                None,
            )
            .await
            .unwrap();
        let text = result.as_text().unwrap();
        assert!(text.contains("Batch tool called"));
        assert!(text.contains("1 calls"));
    }
}