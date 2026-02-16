use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::r#trait::Tool;

/// Central registry for managing a collection of tools.
///
/// Stores tools by name in a HashMap and provides registration, listing,
/// and calling functionality. Used by AggregateToolSource to implement ToolSource trait.
///
/// # Examples
///
/// ```no_run
/// use graphweave::tools::{Tool, ToolRegistry};
/// use graphweave::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
/// use serde_json::json;
/// # use async_trait::async_trait;
/// # struct MockTool;
/// # #[async_trait] impl Tool for MockTool {
/// #     fn name(&self) -> &str { "mock" }
/// #     fn spec(&self) -> ToolSpec { todo!() }
/// #     async fn call(&self, _: serde_json::Value, _: Option<&ToolCallContext>) -> Result<ToolCallContent, ToolSourceError> { todo!() }
/// # }
///
/// let mut registry = ToolRegistry::new();
/// registry.register(Box::new(MockTool));
/// let specs = registry.list();
/// assert_eq!(specs.len(), 1);
/// ```
///
/// # Interaction
///
/// - **Tool**: Stores Box<dyn Tool> instances in HashMap
/// - **ToolRegistryLocked**: Wraps this with Arc<RwLock<Self>> for thread-safe async access
/// - **AggregateToolSource**: Delegates list_tools() and call_tool() to this registry
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Creates a new empty tool registry.
    ///
    /// # Examples
    ///
    /// ```
    /// use graphweave::tools::ToolRegistry;
    ///
    /// let registry = ToolRegistry::new();
    /// assert_eq!(registry.list().len(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Registers a tool in the registry synchronously.
    ///
    /// If a tool with the same name already exists, it will be replaced.
    ///
    /// # Parameters
    ///
    /// - `tool`: Box<dyn Tool> to register
    ///
    /// # Examples
    ///
    /// ```
    /// use graphweave::tools::{Tool, ToolRegistry};
    /// use graphweave::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
    /// # use async_trait::async_trait;
    /// # struct MockTool;
    /// # #[async_trait] impl Tool for MockTool {
    /// #     fn name(&self) -> &str { "mock" }
    /// #     fn spec(&self) -> ToolSpec { todo!() }
    /// #     async fn call(&self, _: serde_json::Value, _: Option<&ToolCallContext>) -> Result<ToolCallContent, ToolSourceError> { todo!() }
    /// # }
    ///
    /// let mut registry = ToolRegistry::new();
    /// registry.register(Box::new(MockTool));
    /// ```
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    /// Lists all registered tools as ToolSpec objects.
    ///
    /// Returns a vector of tool specifications that can be sent to the LLM.
    ///
    /// # Returns
    ///
    /// Vector of ToolSpec for all registered tools.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use graphweave::tools::{Tool, ToolRegistry};
    /// use graphweave::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
    /// # use async_trait::async_trait;
    /// # struct MockTool;
    /// # #[async_trait] impl Tool for MockTool {
    /// #     fn name(&self) -> &str { "mock" }
    /// #     fn spec(&self) -> ToolSpec { todo!() }
    /// #     async fn call(&self, _: serde_json::Value, _: Option<&ToolCallContext>) -> Result<ToolCallContent, ToolSourceError> { todo!() }
    /// # }
    ///
    /// let mut registry = ToolRegistry::new();
    /// registry.register(Box::new(MockTool));
    /// let specs = registry.list();
    /// assert_eq!(specs.len(), 1);
    /// assert_eq!(specs[0].name, "mock");
    /// ```
    pub fn list(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    /// Calls a tool by name with the given arguments and optional context.
    ///
    /// # Parameters
    ///
    /// - `name`: Name of the tool to call
    /// - `args`: JSON arguments to pass to the tool
    /// - `ctx`: Optional per-call context (e.g., recent messages)
    ///
    /// # Returns
    ///
    /// ToolCallContent with the result of tool execution.
    ///
    /// # Errors
    ///
    /// Returns ToolSourceError::NotFound if tool name is not registered.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use graphweave::tools::{Tool, ToolRegistry};
    /// use graphweave::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
    /// use serde_json::json;
    /// # use async_trait::async_trait;
    /// # struct MockTool;
    /// # #[async_trait] impl Tool for MockTool {
    /// #     fn name(&self) -> &str { "mock" }
    /// #     fn spec(&self) -> ToolSpec { todo!() }
    /// #     async fn call(&self, _: serde_json::Value, _: Option<&ToolCallContext>) -> Result<ToolCallContent, ToolSourceError> { todo!() }
    /// # }
    /// # #[tokio::main]
    /// # async fn main() {
    /// let mut registry = ToolRegistry::new();
    /// registry.register(Box::new(MockTool));
    /// let result = registry.call("mock", json!({}), None).await;
    /// # }
    /// ```
    pub async fn call(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| ToolSourceError::NotFound(name.to_string()))?;
        tool.call(args, ctx).await
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe wrapper around ToolRegistry with async RwLock.
///
/// Provides the same interface as ToolRegistry but is safe to share
/// across threads and use with async/await. This is used by AggregateToolSource.
///
/// # Examples
///
/// ```no_run
/// use graphweave::tools::{Tool, ToolRegistryLocked};
/// use graphweave::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
/// use serde_json::json;
/// # use async_trait::async_trait;
/// # struct MockTool;
/// # #[async_trait] impl Tool for MockTool {
/// #     fn name(&self) -> &str { "mock" }
/// #     fn spec(&self) -> ToolSpec { todo!() }
/// #     async fn call(&self, _: serde_json::Value, _: Option<&ToolCallContext>) -> Result<ToolCallContent, ToolSourceError> { todo!() }
/// # }
/// # #[tokio::main]
/// # async fn main() {
/// use std::sync::Arc;
///
/// let mut registry = ToolRegistryLocked::new();
/// registry.register_sync(Box::new(MockTool));
/// let specs = registry.list().await;
/// assert_eq!(specs.len(), 1);
///
/// let shared = Arc::new(registry);
/// let result = shared.call("mock", json!({}), None).await;
/// # }
/// ```
///
/// # Interaction
///
/// - **ToolRegistry**: Inner registry wrapped in Arc<RwLock<...>>
/// - **AggregateToolSource**: Holds this and delegates ToolSource trait methods
/// - **RwLock**: Allows concurrent reads (list) and exclusive writes (register, call)
pub struct ToolRegistryLocked {
    inner: Arc<RwLock<ToolRegistry>>,
}

impl ToolRegistryLocked {
    /// Creates a new empty thread-safe tool registry.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use graphweave::tools::ToolRegistryLocked;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let registry = ToolRegistryLocked::new();
    /// assert_eq!(registry.list().await.len(), 0);
    /// # }
    /// ```
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ToolRegistry::new())),
        }
    }

    /// Registers a tool in the registry asynchronously.
    ///
    /// Prefer this when calling from async context (e.g. during initialization) to avoid
    /// blocking the tokio worker. Does not spawn threads or block.
    ///
    /// # Parameters
    ///
    /// - `tool`: Box<dyn Tool> to register
    pub async fn register_async(&self, tool: Box<dyn Tool>) {
        let mut inner = self.inner.write().await;
        inner.register(tool);
    }

    /// Registers a tool in the registry synchronously.
    ///
    /// This method spawns a new thread with its own tokio runtime to avoid conflicts.
    /// This is useful for constructors where you don't have an async context.
    /// Note: This blocks until registration is complete. Prefer [`register_async`](Self::register_async)
    /// when in async context.
    ///
    /// # Parameters
    ///
    /// - `tool`: Box<dyn Tool> to register
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use graphweave::tools::{Tool, ToolRegistryLocked};
    /// use graphweave::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
    /// # use async_trait::async_trait;
    /// # struct MockTool;
    /// # #[async_trait] impl Tool for MockTool {
    /// #     fn name(&self) -> &str { "mock" }
    /// #     fn spec(&self) -> ToolSpec { todo!() }
    /// #     async fn call(&self, _: serde_json::Value, _: Option<&ToolCallContext>) -> Result<ToolCallContent, ToolSourceError> { todo!() }
    /// # }
    /// # #[tokio::main]
    /// # async fn main() {
    /// let registry = ToolRegistryLocked::new();
    /// registry.register_sync(Box::new(MockTool));
    /// # }
    /// ```
    pub fn register_sync(&self, tool: Box<dyn Tool>) {
        let registry = self.inner.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let mut inner = registry.write().await;
                inner.register(tool);
            });
        })
        .join()
        .expect("Failed to join registration thread");
    }

    /// Lists all registered tools as ToolSpec objects.
    ///
    /// This method acquires a read lock on the inner registry.
    ///
    /// # Returns
    ///
    /// Vector of ToolSpec for all registered tools.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use graphweave::tools::{Tool, ToolRegistryLocked};
    /// use graphweave::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
    /// # use async_trait::async_trait;
    /// # struct MockTool;
    /// # #[async_trait] impl Tool for MockTool {
    /// #     fn name(&self) -> &str { "mock" }
    /// #     fn spec(&self) -> ToolSpec { todo!() }
    /// #     async fn call(&self, _: serde_json::Value, _: Option<&ToolCallContext>) -> Result<ToolCallContent, ToolSourceError> { todo!() }
    /// # }
    /// # #[tokio::main]
    /// # async fn main() {
    /// let mut registry = ToolRegistryLocked::new();
    /// registry.register_sync(Box::new(MockTool));
    /// let specs = registry.list().await;
    /// assert_eq!(specs.len(), 1);
    /// # }
    /// ```
    pub async fn list(&self) -> Vec<ToolSpec> {
        let inner = self.inner.read().await;
        inner.list()
    }

    /// Calls a tool by name with the given arguments and optional context.
    ///
    /// This method acquires a read lock on the inner registry.
    ///
    /// # Parameters
    ///
    /// - `name`: Name of the tool to call
    /// - `args`: JSON arguments to pass to the tool
    /// - `ctx`: Optional per-call context (e.g., recent messages)
    ///
    /// # Returns
    ///
    /// ToolCallContent with the result of tool execution.
    ///
    /// # Errors
    ///
    /// Returns ToolSourceError::NotFound if tool name is not registered.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use graphweave::tools::{Tool, ToolRegistryLocked};
    /// use graphweave::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
    /// use serde_json::json;
    /// # use async_trait::async_trait;
    /// # struct MockTool;
    /// # #[async_trait] impl Tool for MockTool {
    /// #     fn name(&self) -> &str { "mock" }
    /// #     fn spec(&self) -> ToolSpec { todo!() }
    /// #     async fn call(&self, _: serde_json::Value, _: Option<&ToolCallContext>) -> Result<ToolCallContent, ToolSourceError> { todo!() }
    /// # }
    /// # #[tokio::main]
    /// # async fn main() {
    /// let mut registry = ToolRegistryLocked::new();
    /// registry.register_sync(Box::new(MockTool));
    /// let result = registry.call("mock", json!({}), None).await;
    /// # }
    /// ```
    pub async fn call(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let inner = self.inner.read().await;
        inner.call(name, args, ctx).await
    }
}

impl Default for ToolRegistryLocked {
    fn default() -> Self {
        Self::new()
    }
}
