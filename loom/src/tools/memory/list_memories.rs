use async_trait::async_trait;

use serde_json::json;

use crate::memory::{Namespace, Store};
use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

/// Tool name for the list_memories operation.
pub const TOOL_LIST_MEMORIES: &str = "list_memories";

/// Tool for listing all memory keys in the current namespace.
///
/// Wraps Store::list() and exposes it as a tool for LLM.
/// Interacts with Store and Namespace to enumerate stored keys in a fixed namespace.
///
/// # Examples
///
/// ```no_run
/// use loom::tools::{ListMemoriesTool, RememberTool, Tool};
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
/// let list = ListMemoriesTool::new(store, namespace);
/// let result = list.call(json!({}), None).await.unwrap();
/// assert!(result.text.contains("coffee"));
/// assert!(result.text.contains("tea"));
/// # }
/// ```
///
/// # Interaction
///
/// - **Store**: Lists keys via Store::list()
/// - **Namespace**: Isolates storage per user/context
/// - **ToolRegistry**: Registers this tool by name "list_memories"
/// - **StoreToolSource**: Uses this tool via AggregateToolSource
pub struct ListMemoriesTool {
    store: std::sync::Arc<dyn Store>,
    namespace: Namespace,
}

impl ListMemoriesTool {
    /// Creates a new ListMemoriesTool with the given store and namespace.
    ///
    /// # Parameters
    ///
    /// - `store`: Arc<dyn Store> for listing keys
    /// - `namespace`: Namespace to isolate storage (e.g., [user_id])
    ///
    /// # Examples
    ///
    /// ```
    /// use loom::tools::memory::ListMemoriesTool;
    /// use loom::memory::{InMemoryStore, Namespace};
    /// use std::sync::Arc;
    ///
    /// let store = Arc::new(InMemoryStore::new());
    /// let namespace = vec!["user-123".to_string()];
    /// let tool = ListMemoriesTool::new(store, namespace);
    /// ```
    pub fn new(store: std::sync::Arc<dyn Store>, namespace: Namespace) -> Self {
        Self { store, namespace }
    }
}

#[async_trait]
impl Tool for ListMemoriesTool {
    fn name(&self) -> &str {
        TOOL_LIST_MEMORIES
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_LIST_MEMORIES.to_string(),
            description: Some(
                "List all memory keys in the current namespace. Call when you need to see what \
                 has been stored before recalling or searching."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn call(
        &self,
        _args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let keys = self
            .store
            .list(&self.namespace)
            .await
            .map_err(|e| match e {
                crate::memory::StoreError::NotFound => {
                    ToolSourceError::NotFound("key not found".to_string())
                }
                crate::memory::StoreError::Serialization(s) => ToolSourceError::InvalidInput(s),
                crate::memory::StoreError::Storage(s) => ToolSourceError::Transport(s),
                crate::memory::StoreError::EmbeddingError(s) => ToolSourceError::Transport(s),
            })?;

        Ok(ToolCallContent {
            text: serde_json::to_string(&keys)
                .map_err(|e| ToolSourceError::InvalidInput(e.to_string()))?,
        })
    }
}
