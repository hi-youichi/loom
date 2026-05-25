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
        ctx: Option<&ToolCallContext>,
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

        let mut handles = Vec::with_capacity(calls.len());
        for (i, call) in calls.iter().enumerate() {
            let tool_name = call
                .get("tool")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ToolSourceError::InvalidInput(format!("call {}: missing 'tool'", i + 1))
                })?
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
            let r = h.await.map_err(|e| {
                ToolSourceError::Transport(format!("batch task join failed: {}", e))
            })?;
            results.push(r);
        }

        let mut text = String::new();
        for (i, name, result) in results {
            text.push_str(&format!("[{}] {}: ", i + 1, name));
            match result {
                Ok(c) => {
                    let t = c.as_text().unwrap_or("(no text content)").trim();
                    if t.len() > 500 {
                        let mut end = 500;
                        while end > 0 && !t.is_char_boundary(end) {
                            end -= 1;
                        }
                        text.push_str(&format!("{}... (truncated)\n", &t[..end]));
                    } else {
                        text.push_str(&format!("{}\n", t));
                    }
                }
                Err(e) => {
                    text.push_str(&format!("error: {}\n", e));
                }
            }
        }

        Ok(ToolCallContent::text(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use async_trait::async_trait;
    use serde_json::json;

    use std::sync::Arc;

    struct TextTool {
        name: String,
        content: ToolCallContent,
    }

    #[async_trait]
    impl Tool for TextTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn spec(&self) -> crate::tool_source::ToolSpec {
            crate::tool_source::ToolSpec {
                name: self.name.clone(),
                description: None,
                input_schema: json!({}),
                output_hint: None,
            }
        }

        async fn call(
            &self,
            _args: serde_json::Value,
            _ctx: Option<&ToolCallContext>,
        ) -> Result<ToolCallContent, ToolSourceError> {
            Ok(self.content.clone())
        }
    }

    struct ErrorTool {
        name: String,
    }

    #[async_trait]
    impl Tool for ErrorTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn spec(&self) -> crate::tool_source::ToolSpec {
            crate::tool_source::ToolSpec {
                name: self.name.clone(),
                description: None,
                input_schema: json!({}),
                output_hint: None,
            }
        }

        async fn call(
            &self,
            _args: serde_json::Value,
            _ctx: Option<&ToolCallContext>,
        ) -> Result<ToolCallContent, ToolSourceError> {
            Err(ToolSourceError::NotFound(self.name.clone()))
        }
    }

    fn make_batch(tools: Vec<Box<dyn Tool>>) -> BatchTool {
        let source = Arc::new(AggregateToolSource::new());
        for t in tools {
            source.register_sync(t);
        }
        BatchTool::new(source)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_calls_array() {
        let batch = make_batch(vec![]);
        let err = batch.call(json!({}), None).await.unwrap_err();
        assert!(err.to_string().contains("missing or invalid 'calls' array"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn empty_calls_array() {
        let batch = make_batch(vec![]);
        let err = batch.call(json!({"calls": []}), None).await.unwrap_err();
        assert!(err.to_string().contains("calls must have 1–25 items"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_tool_name_in_call() {
        let batch = make_batch(vec![]);
        let err = batch
            .call(json!({"calls": [{"parameters": {}}]}), None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("missing 'tool'"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn successful_text_results() {
        let batch = make_batch(vec![
            Box::new(TextTool {
                name: "echo_a".into(),
                content: ToolCallContent::text("result A"),
            }),
            Box::new(TextTool {
                name: "echo_b".into(),
                content: ToolCallContent::text("result B"),
            }),
        ]);
        let result = batch
            .call(
                json!({"calls": [
                    {"tool": "echo_a", "parameters": {}},
                    {"tool": "echo_b", "parameters": {}}
                ]}),
                None,
            )
            .await
            .unwrap();
        let text = result.as_text().unwrap();
        assert!(text.contains("[1] echo_a: result A"));
        assert!(text.contains("[2] echo_b: result B"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn non_text_content_returns_fallback() {
        let batch = make_batch(vec![
            Box::new(TextTool {
                name: "diff_tool".into(),
                content: ToolCallContent::diff("src/main.rs", None, String::from("new content")),
            }),
            Box::new(TextTool {
                name: "terminal_tool".into(),
                content: ToolCallContent::terminal("term-1"),
            }),
        ]);
        let result = batch
            .call(
                json!({"calls": [
                    {"tool": "diff_tool", "parameters": {}},
                    {"tool": "terminal_tool", "parameters": {}}
                ]}),
                None,
            )
            .await
            .unwrap();
        let text = result.as_text().unwrap();
        assert!(
            text.contains("[1] diff_tool: (no text content)"),
            "expected fallback for Diff variant, got: {}",
            text
        );
        assert!(
            text.contains("[2] terminal_tool: (no text content)"),
            "expected fallback for Terminal variant, got: {}",
            text
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn error_result_in_batch() {
        let batch = make_batch(vec![
            Box::new(TextTool {
                name: "ok_tool".into(),
                content: ToolCallContent::text("ok"),
            }),
            Box::new(ErrorTool {
                name: "fail_tool".into(),
            }),
        ]);
        let result = batch
            .call(
                json!({"calls": [
                    {"tool": "ok_tool", "parameters": {}},
                    {"tool": "fail_tool", "parameters": {}}
                ]}),
                None,
            )
            .await
            .unwrap();
        let text = result.as_text().unwrap();
        assert!(text.contains("[1] ok_tool: ok"));
        assert!(text.contains("[2] fail_tool: error:"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn long_text_is_truncated() {
        let long_content = "x".repeat(600);
        let batch = make_batch(vec![Box::new(TextTool {
            name: "long_tool".into(),
            content: ToolCallContent::text(long_content),
        })]);
        let result = batch
            .call(
                json!({"calls": [{"tool": "long_tool", "parameters": {}}]}),
                None,
            )
            .await
            .unwrap();
        let text = result.as_text().unwrap();
        assert!(text.contains("... (truncated)"));
        assert!(!text.contains(&"x".repeat(600)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mixed_text_and_non_text_and_error() {
        let batch = make_batch(vec![
            Box::new(TextTool {
                name: "text_tool".into(),
                content: ToolCallContent::text("hello"),
            }),
            Box::new(TextTool {
                name: "diff_tool".into(),
                content: ToolCallContent::diff("file.rs", Some(String::from("old")), String::from("new")),
            }),
            Box::new(ErrorTool {
                name: "err_tool".into(),
            }),
        ]);
        let result = batch
            .call(
                json!({"calls": [
                    {"tool": "text_tool", "parameters": {}},
                    {"tool": "diff_tool", "parameters": {}},
                    {"tool": "err_tool", "parameters": {}}
                ]}),
                None,
            )
            .await
            .unwrap();
        let text = result.as_text().unwrap();
        assert!(text.contains("[1] text_tool: hello"));
        assert!(text.contains("[2] diff_tool: (no text content)"));
        assert!(text.contains("[3] err_tool: error:"));
    }
}
