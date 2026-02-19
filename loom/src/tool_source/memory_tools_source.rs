//! Composite tool source: long-term (Store) + short-term (get_recent_messages) in one.
//!
//! Uses AggregateToolSource internally to combine memory and conversation tools.
//! Forwards `set_call_context` to the underlying source for get_recent_messages.

use std::sync::Arc;

use async_trait::async_trait;

use crate::memory::{Namespace, Store};
use crate::tool_source::{ToolSource, ToolSourceError};
use crate::tools::{
    AggregateToolSource, GetRecentMessagesTool, ListMemoriesTool, RecallTool, RememberTool,
    SearchMemoriesTool,
};

/// Composite tool source that exposes both long-term (Store) and short-term (recent messages) memory tools.
///
/// Uses AggregateToolSource internally to register all memory tools and the conversation tool.
/// `list_tools` returns all 5 tools; `call_tool` delegates to the registry;
/// `set_call_context` stores context for get_recent_messages to use.
///
/// **Interaction**: Use with `ActNode::new(Box::new(MemoryToolsSource::new(store, namespace)))`
/// when you want both remember/recall/search_memories/list_memories and get_recent_messages.
pub struct MemoryToolsSource {
    _source: AggregateToolSource,
}

impl MemoryToolsSource {
    /// Creates a composite with both long-term (store + namespace) and short-term memory tools.
    ///
    /// Returns an AggregateToolSource that you can use directly with ActNode.
    /// Note: This function is async and must be awaited.
    ///
    /// # Parameters
    ///
    /// - `store`: Arc<dyn Store> for long-term memory operations
    /// - `namespace`: Namespace to isolate storage (e.g., [user_id])
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use loom::tool_source::MemoryToolsSource;
    /// use loom::memory::{InMemoryStore, Namespace};
    /// use std::sync::Arc;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let store = Arc::new(InMemoryStore::new());
    /// let namespace = vec!["user-123".to_string()];
    /// let source = MemoryToolsSource::new(store, namespace).await;
    /// # }
    /// ```
    #[allow(clippy::new_ret_no_self)]
    pub async fn new(store: Arc<dyn Store>, namespace: Namespace) -> AggregateToolSource {
        let source = AggregateToolSource::new();

        let remember = RememberTool::new(store.clone(), namespace.clone());
        let recall = RecallTool::new(store.clone(), namespace.clone());
        let search = SearchMemoriesTool::new(store.clone(), namespace.clone());
        let list = ListMemoriesTool::new(store, namespace);
        let get_recent = GetRecentMessagesTool::new();

        source.register_async(Box::new(remember)).await;
        source.register_async(Box::new(recall)).await;
        source.register_async(Box::new(search)).await;
        source.register_async(Box::new(list)).await;
        source.register_async(Box::new(get_recent)).await;

        source
    }
}

#[async_trait]
impl ToolSource for MemoryToolsSource {
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
        self._source
            .call_tool_with_context(name, arguments, ctx)
            .await
    }

    fn set_call_context(&self, ctx: Option<crate::tool_source::ToolCallContext>) {
        self._source.set_call_context(ctx)
    }
}
