//! Telegram tools source: send messages, polls, and documents via Telegram Bot API.
//!
//! Uses `AggregateToolSource` internally to register Telegram tools.

use async_trait::async_trait;

use crate::tool_source::{ToolSource, ToolSourceError};
use crate::tools::{
    AggregateToolSource, TelegramSendDocumentTool, TelegramSendMessageTool, TelegramSendPollTool,
};

/// Tool source that exposes Telegram tools: send_message, send_poll, send_document.
///
/// Uses [`AggregateToolSource`] internally to register Telegram tools.
/// Provides a convenient way to enable Telegram capabilities in agents.
///
/// Note: Requires `set_telegram_api` to be called before tool execution.
pub struct TelegramToolsSource {
    _source: AggregateToolSource,
}

impl TelegramToolsSource {
    /// Creates a Telegram tools source.
    ///
    /// Returns an [`AggregateToolSource`] that you can use with [`ActNode`](crate::agent::react::ActNode).
    /// This function is async and must be awaited.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use loom::tool_source::TelegramToolsSource;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let source = TelegramToolsSource::new().await;
    /// # }
    /// ```
    #[allow(clippy::new_ret_no_self)]
    pub async fn new() -> AggregateToolSource {
        let source = AggregateToolSource::new();
        source.register_async(Box::new(TelegramSendMessageTool)).await;
        source.register_async(Box::new(TelegramSendPollTool)).await;
        source.register_async(Box::new(TelegramSendDocumentTool)).await;
        source
    }
}

#[async_trait]
impl ToolSource for TelegramToolsSource {
    /// Lists all registered tools.
    ///
    /// Delegates to [`AggregateToolSource::list_tools`].
    async fn list_tools(&self) -> Result<Vec<crate::tool_source::ToolSpec>, ToolSourceError> {
        self._source.list_tools().await
    }

    /// Calls a tool by name with given arguments.
    ///
    /// Delegates to [`AggregateToolSource::call_tool`].
    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        self._source.call_tool(name, arguments).await
    }

    /// Calls a tool by name with given arguments and optional context.
    ///
    /// Delegates to [`AggregateToolSource::call_tool_with_context`].
    async fn call_tool_with_context(
        &self,
        name: &str,
        arguments: serde_json::Value,
        ctx: Option<&crate::tool_source::ToolCallContext>,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        self._source.call_tool_with_context(name, arguments, ctx).await
    }

    /// Sets the call context for this source.
    ///
    /// Forwards to [`AggregateToolSource`].
    fn set_call_context(&self, ctx: Option<crate::tool_source::ToolCallContext>) {
        self._source.set_call_context(ctx)
    }
}
