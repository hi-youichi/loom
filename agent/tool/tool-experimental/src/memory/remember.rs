use async_trait::async_trait;

use serde_json::json;

use checkpoint::{Namespace, Store};
use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError, Tool};

/// Tool name for the remember operation.
pub use tool_core::tool_name::TOOL_REMEMBER;

/// Tool for writing key-value pairs to long-term memory.
///
/// Wraps Store::put() and exposes it as a tool for the LLM.
/// Interacts with Store and Namespace to persist data in a fixed namespace.
///
/// # Examples
///
/// ```no_run
/// use loom_tools::tools::{RememberTool, Tool};
/// use checkpoint::{InMemoryStore, Namespace};
/// use std::sync::Arc;
/// use serde_json::json;
///
/// # #[tokio::main]
/// # async fn main() {
/// let store = Arc::new(InMemoryStore::new());
/// let namespace = vec!["user-123".to_string()];
/// let tool = RememberTool::new(store.clone(), namespace.clone());
///
/// let args = json!({
///     "key": "preference",
///     "value": "likes coffee"
/// });
/// let result = tool.call(args, None).await.unwrap();
/// assert_eq!(result.as_text().unwrap(), "ok");
/// # }
/// ```
///
/// # Interaction
///
/// - **Store**: Stores key-value pairs via Store::put()
/// - **Namespace**: Isolates storage per user/context
/// - **ToolRegistry**: Registers this tool by name "remember"
/// - **StoreToolSource**: Uses this tool via AggregateToolSource
pub struct RememberTool {
    store: std::sync::Arc<dyn Store>,
    namespace: Namespace,
}

impl RememberTool {
    /// Creates a new RememberTool with the given store and namespace.
    ///
    /// # Parameters
    ///
    /// - `store`: Arc<dyn Store> for persisting key-value pairs
    /// - `namespace`: Namespace to isolate storage (e.g., [user_id])
    ///
    /// # Examples
    ///
    /// ```
    /// use loom_tools::tools::RememberTool;
    /// use checkpoint::{InMemoryStore, Namespace};
    /// use std::sync::Arc;
    ///
    /// let store = Arc::new(InMemoryStore::new());
    /// let namespace = vec!["user-123".to_string()];
    /// let tool = RememberTool::new(store, namespace);
    /// ```
    pub fn new(store: std::sync::Arc<dyn Store>, namespace: Namespace) -> Self {
        Self { store, namespace }
    }
}

#[async_trait]
impl Tool for RememberTool {
    fn name(&self) -> &str {
        TOOL_REMEMBER
    }

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
            name: TOOL_REMEMBER.to_string(),
            description: Some(
                "Write a key-value pair to long-term memory. Call when: the user expresses a preference, \
                 the user explicitly asks to remember something, or existing memory should be updated.".to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Memory key" },
                    "value": { "description": "Value (any JSON)" }
                },
                "required": ["key", "value"]
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
        let value = args
            .get("value")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        self.store
            .put(&self.namespace, key, &value)
            .await
            .map_err(|e| match e {
                checkpoint::StoreError::NotFound => {
                    ToolSourceError::NotFound("key not found".to_string())
                }
                checkpoint::StoreError::Serialization(s) => ToolSourceError::InvalidInput(s),
                checkpoint::StoreError::Storage(s) => ToolSourceError::Transport(s),
                checkpoint::StoreError::EmbeddingError(s) => ToolSourceError::Transport(s),
            })?;

        Ok(ToolCallContent::text("ok".to_string()))
    }
}
