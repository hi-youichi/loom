//! Dry-run tool source: delegates list_tools, returns a placeholder for calls so tools are not executed.

use super::{ToolCallContent, ToolCallContext, ToolSource, ToolSourceError, ToolSpec};
use async_trait::async_trait;
use serde_json::Value;

/// Wraps a `ToolSource` and returns a placeholder result for every tool call instead of executing.
/// Used when `--dry` is set: LLM runs and may request tools, but no side effects occur.
pub struct DryRunToolSource {
    inner: Box<dyn ToolSource>,
}

impl DryRunToolSource {
    pub fn new(inner: Box<dyn ToolSource>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl ToolSource for DryRunToolSource {
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError> {
        self.inner.list_tools().await
    }

    async fn call_tool(
        &self,
        name: &str,
        _arguments: Value,
    ) -> Result<ToolCallContent, ToolSourceError> {
        Ok(ToolCallContent::text(format!(
            "(dry run: {} was not executed)",
            name
        )))
    }

    async fn call_tool_with_context(
        &self,
        name: &str,
        _arguments: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        Ok(ToolCallContent::text(format!(
            "(dry run: {} was not executed)",
            name
        )))
    }

    fn set_call_context(&self, ctx: Option<ToolCallContext>) {
        self.inner.set_call_context(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::{DryRunToolSource, ToolCallContent, ToolSource};
    use crate::tool_source::MockToolSource;

    #[tokio::test]
    async fn dry_run_delegates_list_tools_returns_placeholder_for_call() {
        let inner = Box::new(MockToolSource::get_time_example());
        let dry = DryRunToolSource::new(inner);

        let tools = dry.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "get_time");

        let out: ToolCallContent = dry
            .call_tool("get_time", serde_json::json!({}))
            .await
            .unwrap();
        assert!(out.as_text().unwrap().contains("dry run"));
        assert!(out.as_text().unwrap().contains("get_time"));
        assert!(out.as_text().unwrap().contains("was not executed"));
        assert!(!out.as_text().unwrap().contains("12:00")); // real mock would return time
    }
}
