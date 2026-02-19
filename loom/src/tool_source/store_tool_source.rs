//! Store-backed tool source: long-term memory as tools (remember, recall, search_memories, list_memories).
//!
//! Wraps `Store` with a fixed namespace and exposes put/get/list/search as tools for the LLM.
//! Uses AggregateToolSource internally to register memory tools.

use std::sync::Arc;

use async_trait::async_trait;

use crate::memory::{Namespace, Store};
use crate::tool_source::{ToolSource, ToolSourceError};
use crate::tools::{
    AggregateToolSource, ListMemoriesTool, RecallTool, RememberTool, SearchMemoriesTool,
};

/// Tool name: write a key-value pair to long-term memory.
pub const TOOL_REMEMBER: &str = "remember";
/// Tool name: read a value by key from long-term memory.
pub const TOOL_RECALL: &str = "recall";
/// Tool name: search memories by query (and optional limit).
pub const TOOL_SEARCH_MEMORIES: &str = "search_memories";
/// Tool name: list all keys in the current namespace.
pub const TOOL_LIST_MEMORIES: &str = "list_memories";

/// Tool source that exposes Store operations as tools (remember, recall, search_memories, list_memories).
///
/// Holds `Arc<dyn Store>` and a fixed namespace (e.g. `[user_id, "memories"]`). Uses AggregateToolSource
/// internally to register memory tools. Use with ActNode or composite ToolSource for long-term memory.
pub struct StoreToolSource {
    _source: AggregateToolSource,
}

impl StoreToolSource {
    /// Creates a store tool source with the given store and namespace.
    ///
    /// Returns an AggregateToolSource that you can use directly with ActNode.
    /// Note: This function is async and must be awaited.
    ///
    /// # Parameters
    ///
    /// - `store`: Arc<dyn Store> for persisting key-value pairs
    /// - `namespace`: Namespace to isolate storage (e.g., [user_id])
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use loom::tool_source::StoreToolSource;
    /// use loom::memory::{InMemoryStore, Namespace};
    /// use std::sync::Arc;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let store = Arc::new(InMemoryStore::new());
    /// let namespace = vec!["user-123".to_string()];
    /// let source = StoreToolSource::new(store, namespace).await;
    /// # }
    /// ```
    #[allow(clippy::new_ret_no_self)]
    pub async fn new(store: Arc<dyn Store>, namespace: Namespace) -> AggregateToolSource {
        let source = AggregateToolSource::new();

        let remember = RememberTool::new(store.clone(), namespace.clone());
        let recall = RecallTool::new(store.clone(), namespace.clone());
        let search = SearchMemoriesTool::new(store.clone(), namespace.clone());
        let list = ListMemoriesTool::new(store, namespace);

        source.register_sync(Box::new(remember));
        source.register_sync(Box::new(recall));
        source.register_sync(Box::new(search));
        source.register_sync(Box::new(list));

        source
    }
}

#[async_trait]
impl ToolSource for StoreToolSource {
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
