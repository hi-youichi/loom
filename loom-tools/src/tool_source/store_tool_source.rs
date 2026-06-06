//! Store-backed tool source: long-term memory as tools (remember, recall, search_memories, list_memories).
//!
//! Wraps `Store` with a fixed namespace and exposes put/get/list/search as tools for the LLM.
//! Uses AggregateToolSource internally to register memory tools.

use std::sync::Arc;

use async_trait::async_trait;

use loom_memory::{Namespace, Store};
use crate::tool_source::{ToolSource, ToolSourceError};
use crate::tools::{
    AggregateToolSource, ListMemoriesTool, RecallTool, RememberTool, SearchMemoriesTool,
};

pub const TOOL_REMEMBER: &str = "remember";
pub const TOOL_RECALL: &str = "recall";
pub const TOOL_SEARCH_MEMORIES: &str = "search_memories";
pub const TOOL_LIST_MEMORIES: &str = "list_memories";

/// Tool source that exposes Store operations as tools (remember, recall, search_memories, list_memories).
///
/// Holds `Arc<dyn Store>` and a fixed namespace (e.g. `[user_id, "memories"]`). Uses AggregateToolSource
/// internally to register memory tools. Use with ActNode or composite ToolSource for long-term memory.
pub struct StoreToolSource {
    _source: AggregateToolSource,
}

impl StoreToolSource {
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
        _ctx: Option<&crate::tool_source::ToolCallContext>,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        self._source.call_tool(name, arguments, _ctx).await
    }
}
