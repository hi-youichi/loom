//! Filtered tool source: wraps a ToolSource and applies enabled/disabled filters.
//!
//! Used by `build_tool_source` to enforce agent-profile tool permissions.
//! Tools not in the whitelist (enabled) or in the blacklist (disabled) are
//! hidden from `list_tools` and blocked on `call_tool`.

use async_trait::async_trait;
use serde_json::Value;

use super::{ToolCallContent, ToolCallContext, ToolSource, ToolSourceError, ToolSpec};
use crate::agent::react::BuiltinToolFilter;

/// A ToolSource wrapper that filters tools by name using a [`BuiltinToolFilter`].
///
/// - `list_tools` returns only specs whose names pass the filter.
/// - `call_tool` / `call_tool_with_context` reject calls to filtered-out tools.
/// - `set_call_context` is forwarded to the inner source.
pub struct FilteredToolSource {
    inner: Box<dyn ToolSource>,
    filter: BuiltinToolFilter,
}

impl FilteredToolSource {
    pub fn new(inner: Box<dyn ToolSource>, filter: BuiltinToolFilter) -> Self {
        Self { inner, filter }
    }
}

#[async_trait]
impl ToolSource for FilteredToolSource {
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError> {
        let all = self.inner.list_tools().await?;
        Ok(all
            .into_iter()
            .filter(|spec| self.filter.is_allowed(&spec.name))
            .collect())
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<ToolCallContent, ToolSourceError> {
        if !self.filter.is_allowed(name) {
            return Err(ToolSourceError::NotFound(format!(
                "tool '{}' is disabled for this agent",
                name
            )));
        }
        self.inner.call_tool(name, arguments).await
    }

    async fn call_tool_with_context(
        &self,
        name: &str,
        arguments: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        if !self.filter.is_allowed(name) {
            return Err(ToolSourceError::NotFound(format!(
                "tool '{}' is disabled for this agent",
                name
            )));
        }
        self.inner.call_tool_with_context(name, arguments, ctx).await
    }

    fn set_call_context(&self, ctx: Option<ToolCallContext>) {
        self.inner.set_call_context(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_source::ToolSpec;

    /// A minimal ToolSource that returns a fixed list of specs and echoes the name on call.
    struct MockSource;

    fn spec(name: &str) -> ToolSpec {
        ToolSpec {
            name: name.to_string(),
            description: None,
            input_schema: serde_json::json!({}),
            output_hint: None,
        }
    }

    #[async_trait]
    impl ToolSource for MockSource {
        async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError> {
            Ok(vec![spec("read"), spec("write_file"), spec("bash"), spec("edit")])
        }
        async fn call_tool(
            &self,
            name: &str,
            _arguments: Value,
        ) -> Result<ToolCallContent, ToolSourceError> {
            Ok(ToolCallContent::text(format!("called {}", name)))
        }
    }

    #[tokio::test]
    async fn enabled_whitelist_filters() {
        let filter = BuiltinToolFilter {
            enabled: Some(vec!["read".into(), "bash".into()]),
            disabled: None,
        };
        let filtered = FilteredToolSource::new(Box::new(MockSource), filter);
        let tools = filtered.list_tools().await.unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, &["read", "bash"]);
    }

    #[tokio::test]
    async fn disabled_blacklist_filters() {
        let filter = BuiltinToolFilter {
            enabled: None,
            disabled: Some(vec!["write_file".into(), "edit".into()]),
        };
        let filtered = FilteredToolSource::new(Box::new(MockSource), filter);
        let tools = filtered.list_tools().await.unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, &["read", "bash"]);
    }

    #[tokio::test]
    async fn combined_enabled_and_disabled() {
        let filter = BuiltinToolFilter {
            enabled: Some(vec!["read".into(), "write_file".into(), "bash".into()]),
            disabled: Some(vec!["write_file".into()]),
        };
        let filtered = FilteredToolSource::new(Box::new(MockSource), filter);
        let tools = filtered.list_tools().await.unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, &["read", "bash"]);
    }

    #[tokio::test]
    async fn call_tool_blocked() {
        let filter = BuiltinToolFilter {
            enabled: None,
            disabled: Some(vec!["write_file".into()]),
        };
        let filtered = FilteredToolSource::new(Box::new(MockSource), filter);
        let result = filtered
            .call_tool("write_file", serde_json::json!({}))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("disabled"), "unexpected error: {}", err);
    }

    #[tokio::test]
    async fn call_tool_allowed() {
        let filter = BuiltinToolFilter {
            enabled: None,
            disabled: Some(vec!["write_file".into()]),
        };
        let filtered = FilteredToolSource::new(Box::new(MockSource), filter);
        let result = filtered
            .call_tool("read", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(result.as_text(), Some("called read"));
    }
}
