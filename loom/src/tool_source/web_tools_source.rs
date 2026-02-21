//! Web tools source: web_fetcher for HTTP GET/POST requests.
//!
//! Uses `AggregateToolSource` internally to register WebFetcherTool.

use async_trait::async_trait;

use crate::tool_source::{ToolSource, ToolSourceError};
use crate::tools::{AggregateToolSource, WebFetcherTool};

/// Tool name: fetch or send content via HTTP GET/POST.
pub const TOOL_WEB_FETCHER: &str = "web_fetcher";

/// Tool source that exposes web fetcher as one tool: web_fetcher.
///
/// Uses AggregateToolSource internally to register WebFetcherTool.
/// Provides a convenient way to enable web fetching capabilities in agents.
pub struct WebToolsSource {
    _source: AggregateToolSource,
}

impl WebToolsSource {
    /// Creates a web tools source.
    ///
    /// Returns an AggregateToolSource that you can use directly with ActNode.
    /// Note: This function is async and must be awaited.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use loom::tool_source::WebToolsSource;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let source = WebToolsSource::new().await;
    /// # }
    /// ```
    #[allow(clippy::new_ret_no_self)]
    pub async fn new() -> AggregateToolSource {
        let source = AggregateToolSource::new();
        source.register_async(Box::new(WebFetcherTool::new())).await;
        source
    }

    /// Creates a web tools source with a custom HTTP client.
    ///
    /// # Parameters
    ///
    /// - `client`: Custom reqwest::Client for configuring timeouts, proxies, etc.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use loom::tool_source::WebToolsSource;
    /// use std::time::Duration;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let client = reqwest::Client::builder()
    ///     .timeout(Duration::from_secs(30))
    ///     .build()
    ///     .unwrap();
    /// let source = WebToolsSource::with_client(client).await;
    /// # }
    /// ```
    #[allow(clippy::new_ret_no_self)]
    pub async fn with_client(client: reqwest::Client) -> AggregateToolSource {
        let source = AggregateToolSource::new();
        source
            .register_async(Box::new(WebFetcherTool::with_client(client)))
            .await;
        source
    }
}

#[async_trait]
impl ToolSource for WebToolsSource {
    /// Lists all registered tools.
    ///
    /// Delegates to AggregateToolSource::list_tools().
    ///
    /// # Returns
    ///
    /// Vector of ToolSpec containing web_fetcher tool.
    ///
    /// # Errors
    ///
    /// Never fails (always returns Ok).
    ///
    /// # Interaction
    ///
    /// - Called by ThinkNode to build tool descriptions for LLM prompts
    /// - Delegates to AggregateToolSource
    async fn list_tools(&self) -> Result<Vec<crate::tool_source::ToolSpec>, ToolSourceError> {
        self._source.list_tools().await
    }

    /// Calls a tool by name with given arguments.
    ///
    /// Delegates to AggregateToolSource::call_tool().
    ///
    /// # Parameters
    ///
    /// - `name`: Name of tool to call (e.g., "web_fetcher")
    /// - `arguments`: JSON arguments to pass to tool
    ///
    /// # Returns
    ///
    /// ToolCallContent with result of tool execution.
    ///
    /// # Errors
    ///
    /// Returns ToolSourceError::NotFound if tool name is not registered,
    /// or any error from tool's call() method.
    ///
    /// # Interaction
    ///
    /// - Called by ActNode when executing tool calls
    /// - Delegates to AggregateToolSource
    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        self._source.call_tool(name, arguments).await
    }

    /// Calls a tool by name with given arguments and optional context.
    ///
    /// Delegates to AggregateToolSource::call_tool_with_context().
    /// WebFetcherTool does not use context, but method is required by ToolSource trait.
    ///
    /// # Parameters
    ///
    /// - `name`: Name of tool to call
    /// - `arguments`: JSON arguments to pass to tool
    /// - `ctx`: Optional context (not used by WebFetcherTool)
    ///
    /// # Returns
    ///
    /// ToolCallContent with result of tool execution.
    ///
    /// # Errors
    ///
    /// Returns ToolSourceError::NotFound if tool name is not registered,
    /// or any error from tool's call() method.
    ///
    /// # Interaction
    ///
    /// - Called by ActNode with ToolCallContext before executing tool calls
    /// - Delegates to AggregateToolSource
    async fn call_tool_with_context(
        &self,
        name: &str,
        arguments: serde_json::Value,
        ctx: Option<&crate::tool_source::ToolCallContext>,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        self._source
            .call_tool_with_context(name, arguments, ctx)
            .await
    }

    /// Sets the call context for this source.
    ///
    /// WebFetcherTool does not use context, but method is required by ToolSource trait.
    /// Forwards to AggregateToolSource for consistency.
    ///
    /// # Parameters
    ///
    /// - `ctx`: Optional ToolCallContext to store
    ///
    /// # Interaction
    ///
    /// - Called by ActNode before tool execution
    /// - Forwards to AggregateToolSource
    fn set_call_context(&self, ctx: Option<crate::tool_source::ToolCallContext>) {
        self._source.set_call_context(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSpec};
    use crate::tools::Tool;

    struct DummyTool;

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "dummy_tool"
        }

        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "dummy_tool".to_string(),
                description: Some("dummy".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "x": { "type": "number" } }
                }),
            }
        }

        async fn call(
            &self,
            args: serde_json::Value,
            _ctx: Option<&ToolCallContext>,
        ) -> Result<ToolCallContent, ToolSourceError> {
            Ok(ToolCallContent {
                text: format!("dummy:{}", args),
            })
        }
    }

    #[tokio::test]
    async fn new_registers_web_fetcher_tool() {
        let source = WebToolsSource::new().await;
        let tools = source.list_tools().await.unwrap();
        assert!(tools.iter().any(|s| s.name == TOOL_WEB_FETCHER));
    }

    #[tokio::test]
    async fn with_client_registers_web_fetcher_tool() {
        let client = reqwest::Client::new();
        let source = WebToolsSource::with_client(client).await;
        let tools = source.list_tools().await.unwrap();
        assert!(tools.iter().any(|s| s.name == TOOL_WEB_FETCHER));
    }

    #[tokio::test]
    async fn web_tools_source_trait_methods_delegate_to_aggregate_source() {
        let aggregate = AggregateToolSource::new();
        aggregate.register_async(Box::new(DummyTool)).await;
        let source = WebToolsSource { _source: aggregate };

        let listed = source.list_tools().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "dummy_tool");

        let called = source
            .call_tool("dummy_tool", serde_json::json!({"x": 1}))
            .await
            .unwrap();
        assert!(called.text.contains("\"x\":1"));

        source.set_call_context(Some(ToolCallContext::default()));
        let called_with_ctx = source
            .call_tool_with_context(
                "dummy_tool",
                serde_json::json!({"x": 2}),
                Some(&ToolCallContext::new(vec![])),
            )
            .await
            .unwrap();
        assert!(called_with_ctx.text.contains("\"x\":2"));
    }
}
