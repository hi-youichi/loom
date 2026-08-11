use async_trait::async_trait;

use serde_json::json;

use checkpoint::{Namespace, Store};
use tool_core::{Tool, ToolCallContent, ToolCallContext, ToolSourceError};

/// Tool name for the search_memories operation.
pub use tool_core::tool_name::TOOL_SEARCH_MEMORIES;

/// Tool for searching long-term memories by query (optional) and limit (optional).
///
/// Wraps Store::search() and exposes it as a tool for LLM.
/// Interacts with Store and Namespace to perform semantic search in a fixed namespace.
///
/// # Examples
///
/// ```no_run
/// use loom_tools::tools::{RememberTool, SearchMemoriesTool, Tool};
/// use checkpoint::{InMemoryStore, Namespace};
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
/// assert!(result.as_text().unwrap().contains("coffee") || result.as_text().unwrap().contains("tea"));
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
    /// use loom_tools::tools::SearchMemoriesTool;
    /// use checkpoint::{InMemoryStore, Namespace};
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

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
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
            output_hint: None,
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
                checkpoint::StoreError::NotFound => {
                    ToolSourceError::NotFound("key not found".to_string())
                }
                checkpoint::StoreError::Serialization(s) => ToolSourceError::InvalidInput(s),
                checkpoint::StoreError::Storage(s) => ToolSourceError::Transport(s),
                checkpoint::StoreError::EmbeddingError(s) => ToolSourceError::Transport(s),
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

        Ok(ToolCallContent::text(serde_json::to_string(&arr).map_err(
            |e| ToolSourceError::InvalidInput(e.to_string()),
        )?))
    }
}
