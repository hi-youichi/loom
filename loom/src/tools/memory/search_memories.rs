use async_trait::async_trait;

use serde_json::json;

use crate::memory::{Namespace, Store};
use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

/// Tool name for the search_memories operation.
pub const TOOL_SEARCH_MEMORIES: &str = "search_memories";

/// Tool for searching long-term memories by query (optional) and limit (optional).
///
/// Wraps Store::search() and exposes it as a tool for LLM.
/// Interacts with Store and Namespace to perform semantic search in a fixed namespace.
///
/// # Examples
///
/// ```no_run
/// use loom::tools::{RememberTool, SearchMemoriesTool, Tool};
/// use loom::memory::{InMemoryStore, Namespace};
/// use std::sync::Arc;
/// use serde_json::json;
///
/// # #[tokio::main]
/// # async fn main() {
/// let store = Arc::new(InMemoryStore::new());
/// let namespace = vec!["user-123".to_string()];
///
/// let remember = RememberTool::new(store.clone(), namespace.clone());
/// remember.call(json!({"key": "coffee", "value": "likes coffee"}), None).await.unwrap();
/// remember.call(json!({"key": "tea", "value": "dislikes tea"}), None).await.unwrap();
///
/// let search = SearchMemoriesTool::new(store, namespace);
/// let result = search.call(json!({"query": "drink preference"}), None).await.unwrap();
/// assert!(result.text.contains("coffee") || result.text.contains("tea"));
/// # }
/// ```
///
/// # Interaction
///
/// - **Store**: Performs semantic search via Store::search()
/// - **Namespace**: Isolates storage per user/context
/// - **ToolRegistry**: Registers this tool by name "search_memories"
/// - **StoreToolSource**: Uses this tool via AggregateToolSource
pub struct SearchMemoriesTool {
    store: std::sync::Arc<dyn Store>,
    namespace: Namespace,
}

impl SearchMemoriesTool {
    /// Creates a new SearchMemoriesTool with the given store and namespace.
    ///
    /// # Parameters
    ///
    /// - `store`: Arc<dyn Store> for performing semantic search
    /// - `namespace`: Namespace to isolate storage (e.g., [user_id])
    ///
    /// # Examples
    ///
    /// ```
    /// use loom::tools::memory::SearchMemoriesTool;
    /// use loom::memory::{InMemoryStore, Namespace};
    /// use std::sync::Arc;
    ///
    /// let store = Arc::new(InMemoryStore::new());
    /// let namespace = vec!["user-123".to_string()];
    /// let tool = SearchMemoriesTool::new(store, namespace);
    /// ```
    pub fn new(store: std::sync::Arc<dyn Store>, namespace: Namespace) -> Self {
        Self { store, namespace }
    }
}

#[async_trait]
impl Tool for SearchMemoriesTool {
    fn name(&self) -> &str {
        TOOL_SEARCH_MEMORIES
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_SEARCH_MEMORIES.to_string(),
            description: Some(
                "Search long-term memories by query (optional) and limit (optional). Call when you need \
                 to find relevant past information before answering or acting.".to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query (optional)" },
                    "limit": { "type": "integer", "description": "Max results (optional)" }
                }
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let query = args.get("query").and_then(|v| v.as_str()).map(String::from);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);

        // Use search_simple for backward compatibility
        let hits = self
            .store
            .search_simple(&self.namespace, query.as_deref(), limit)
            .await
            .map_err(|e| match e {
                crate::memory::StoreError::NotFound => {
                    ToolSourceError::NotFound("key not found".to_string())
                }
                crate::memory::StoreError::Serialization(s) => ToolSourceError::InvalidInput(s),
                crate::memory::StoreError::Storage(s) => ToolSourceError::Transport(s),
                crate::memory::StoreError::EmbeddingError(s) => ToolSourceError::Transport(s),
            })?;

        let arr: Vec<serde_json::Value> = hits
            .into_iter()
            .map(|h| {
                json!({
                    "key": h.key,
                    "value": h.value,
                    "score": h.score
                })
            })
            .collect();

        Ok(ToolCallContent {
            text: serde_json::to_string(&arr)
                .map_err(|e| ToolSourceError::InvalidInput(e.to_string()))?,
        })
    }
}
