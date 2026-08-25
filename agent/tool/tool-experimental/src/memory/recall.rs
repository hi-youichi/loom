use async_trait::async_trait;

use serde_json::json;

use checkpoint::{Namespace, Store};
use tool_core::{Tool, ToolCallContent, ToolCallContext, ToolSourceError};

/// Tool name for the recall operation.
pub use tool_core::tool_name::TOOL_RECALL;

/// Tool for reading a value by key from long-term memory.
///
/// Wraps Store::get() and exposes it as a tool for LLM.
/// Interacts with Store and Namespace to retrieve data from a fixed namespace.
///
/// # Examples
///
/// ```no_run
/// use anureo_tools::tools::{RecallTool, RememberTool, Tool};
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
/// remember.call(json!({"key": "preference", "value": "likes coffee"}), None).await.unwrap();
///
/// let recall = RecallTool::new(store, namespace);
/// let result = recall.call(json!({"key": "preference"}), None).await.unwrap();
/// assert!(result.as_text().unwrap().contains("likes coffee"));
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
    /// use anureo_tools::tools::RecallTool;
    /// use checkpoint::{InMemoryStore, Namespace};
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

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
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
            output_hint: None,
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
                checkpoint::StoreError::NotFound => {
                    ToolSourceError::NotFound("key not found".to_string())
                }
                checkpoint::StoreError::Serialization(s) => ToolSourceError::InvalidInput(s),
                checkpoint::StoreError::Storage(s) => ToolSourceError::Transport(s),
                checkpoint::StoreError::EmbeddingError(s) => ToolSourceError::Transport(s),
            })?;

        let text = match opt {
            Some(v) => v.to_string(),
            None => return Err(ToolSourceError::NotFound("key not found".to_string())),
        };

        Ok(ToolCallContent::text(text))
    }
}
