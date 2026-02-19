//! Short-term memory tool source: get_recent_messages from current step context.
//!
//! Uses `ToolCallContext` (injected by ActNode via `set_call_context`) to return
//! last N messages. Uses AggregateToolSource internally to register get_recent_messages tool.

use std::sync::RwLock;

use async_trait::async_trait;

use crate::tool_source::{ToolSource, ToolSourceError};
use crate::tools::{AggregateToolSource, GetRecentMessagesTool};

/// Tool name: get recent messages from current conversation.
pub const TOOL_GET_RECENT_MESSAGES: &str = "get_recent_messages";

/// Tool source that exposes current-step messages as one tool: get_recent_messages.
///
/// Uses AggregateToolSource internally to register GetRecentMessagesTool. Stores context in
/// RwLock<Option<ToolCallContext>>; ActNode calls `set_call_context` before tool execution.
pub struct ShortTermMemoryToolSource {
    context: RwLock<Option<crate::tool_source::ToolCallContext>>,
    _source: AggregateToolSource,
}

impl ShortTermMemoryToolSource {
    /// Creates a short-term memory tool source.
    ///
    /// Returns an AggregateToolSource that you can use directly with ActNode.
    /// Note: This function is async and must be awaited.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use loom::tool_source::ShortTermMemoryToolSource;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let source = ShortTermMemoryToolSource::new().await;
    /// # }
    /// ```
    #[allow(clippy::new_ret_no_self)]
    pub async fn new() -> AggregateToolSource {
        let source = AggregateToolSource::new();
        source
            .register_async(Box::new(GetRecentMessagesTool::new()))
            .await;
        source
    }
}

#[async_trait]
impl ToolSource for ShortTermMemoryToolSource {
    async fn list_tools(&self) -> Result<Vec<crate::tool_source::ToolSpec>, ToolSourceError> {
        self._source.list_tools().await
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        self._source.call_tool(name, arguments).await
    }

    async fn call_tool_with_context(
        &self,
        name: &str,
        arguments: serde_json::Value,
        ctx: Option<&crate::tool_source::ToolCallContext>,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        if let Some(c) = ctx {
            if let Ok(mut g) = self.context.write() {
                *g = Some(c.clone());
            }
        }
        self._source
            .call_tool_with_context(name, arguments, ctx)
            .await
    }

    fn set_call_context(&self, ctx: Option<crate::tool_source::ToolCallContext>) {
        if let Ok(mut g) = self.context.write() {
            *g = ctx;
        }
    }
}
