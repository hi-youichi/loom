//! Batch tool: run multiple independent tool calls in parallel (1–25 per batch).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSource, ToolSourceError};
use crate::tools::{AggregateToolSource, Tool};

/// Tool name for batch execution.
pub const TOOL_BATCH: &str = "batch";

const MAX_CALLS: usize = 25;

/// Tool that executes multiple tool calls in parallel.
pub struct BatchTool {
    source: Arc<AggregateToolSource>,
}

impl BatchTool {
    pub fn new(source: Arc<AggregateToolSource>) -> Self {
        Self { source }
    }
}

#[async_trait]
impl Tool for BatchTool {
    fn name(&self) -> &str {
        TOOL_BATCH
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
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
                                "tool": { "type": "string", "description": "Tool name (e.g. read, grep, bash)." },
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
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let calls = args
            .get("calls")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing or invalid 'calls' array".to_string()))?;

        if calls.is_empty() || calls.len() > MAX_CALLS {
            return Err(ToolSourceError::InvalidInput(format!(
                "calls must have 1–{} items, got {}",
                MAX_CALLS,
                calls.len()
            )));
        }

        let mut handles = Vec::with_capacity(calls.len());
        for (i, call) in calls.iter().enumerate() {
            let tool_name = call
                .get("tool")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolSourceError::InvalidInput(format!("call {}: missing 'tool'", i + 1)))?
                .to_string();
            let params = call
                .get("parameters")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            let source = Arc::clone(&self.source);
            let ctx_clone = ctx.cloned();
            handles.push(tokio::spawn(async move {
                let ctx_ref = ctx_clone.as_ref();
                let out = source
                    .call_tool_with_context(&tool_name, params, ctx_ref)
                    .await;
                (i, tool_name, out)
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for h in handles {
            let r = h
                .await
                .map_err(|e| ToolSourceError::Transport(format!("batch task join failed: {}", e)))?;
            results.push(r);
        }

        let mut text = String::new();
        for (i, name, result) in results {
            text.push_str(&format!("[{}] {}: ", i + 1, name));
            match result {
                Ok(c) => {
                    let t = c.text.trim();
                    if t.len() > 500 {
                        text.push_str(&format!("{}... (truncated)\n", &t[..500]));
                    } else {
                        text.push_str(&format!("{}\n", t));
                    }
                }
                Err(e) => {
                    text.push_str(&format!("error: {}\n", e));
                }
            }
        }

        Ok(ToolCallContent { text })
    }
}
