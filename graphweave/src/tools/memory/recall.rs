use async_trait::async_trait;

use serde_json::json;

use crate::memory::{Namespace, Store};
use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

/// Tool name for the recall operation.
pub const TOOL_RECALL: &str = "recall";

/// Tool for reading a value by key from long-term memory.
///
/// Wraps Store::get() and exposes it as a tool for LLM.
/// Interacts with Store and Namespace to retrieve data from a fixed namespace.
///
/// # Examples
///
/// ```no_run
/// use graphweave::tools::{RecallTool, RememberTool, Tool};
/// use graphweave::memory::{InMemoryStore, Namespace};
/// use std::sync::Arc;
/// use serde_json::json;
///
/// # #[tokio::main]
/// # async fn main() {
/// let store = Arc::new(InMemoryStore::new());
/// let namespace = vec!["user-123".to_string()];
///
/// let remember = RememberTool::new(store.clone(), namespace.clone());
/// remember.call(json!({"key": "preference", "value": "likes coffee"}), None).await.unwrap();
///
/// let recall = RecallTool::new(store, namespace);
/// let result = recall.call(json!({"key": "preference"}), None).await.unwrap();
/// assert!(result.text.contains("likes coffee"));
/// # }
/// ```
///
/// # Interaction
///
/// - **Store**: Retrieves values via Store::get()
/// - **Namespace**: Isolates storage per user/context
/// - **ToolRegistry**: Registers this tool by name "recall"
/// - **StoreToolSource**: Uses this tool via AggregateToolSource
pub struct RecallTool {
    store: std::sync::Arc<dyn Store>,
    namespace: Namespace,
}

impl RecallTool {
    /// Creates a new RecallTool with the given store and namespace.
    ///
    /// # Parameters
    ///
    /// - `store`: Arc<dyn Store> for retrieving key-value pairs
    /// - `namespace`: Namespace to isolate storage (e.g., [user_id])
    ///
    /// # Examples
    ///
    /// ```
    /// use graphweave::tools::memory::RecallTool;
    /// use graphweave::memory::{InMemoryStore, Namespace};
    /// use std::sync::Arc;
    ///
    /// let store = Arc::new(InMemoryStore::new());
    /// let namespace = vec!["user-123".to_string()];
    /// let tool = RecallTool::new(store, namespace);
    /// ```
    pub fn new(store: std::sync::Arc<dyn Store>, namespace: Namespace) -> Self {
        Self { store, namespace }
    }
}

#[async_trait]
impl Tool for RecallTool {
    fn name(&self) -> &str {
        TOOL_RECALL
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_RECALL.to_string(),
            description: Some(
                "Read a value by key from long-term memory. Call when you need to retrieve something \
                 previously stored with remember.".to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Memory key" }
                },
                "required": ["key"]
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let key = args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing key".to_string()))?;

        let opt = self
            .store
            .get(&self.namespace, key)
            .await
            .map_err(|e| match e {
                crate::memory::StoreError::NotFound => {
                    ToolSourceError::NotFound("key not found".to_string())
                }
                crate::memory::StoreError::Serialization(s) => ToolSourceError::InvalidInput(s),
                crate::memory::StoreError::Storage(s) => ToolSourceError::Transport(s),
                crate::memory::StoreError::EmbeddingError(s) => ToolSourceError::Transport(s),
            })?;

        let text = match opt {
            Some(v) => v.to_string(),
            None => return Err(ToolSourceError::NotFound("key not found".to_string())),
        };

        Ok(ToolCallContent { text })
    }
}
