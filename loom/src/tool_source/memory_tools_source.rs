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
        _ctx: Option<&crate::tool_source::ToolCallContext>,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        self._source.call_tool(name, arguments, _ctx).await
    }
}
