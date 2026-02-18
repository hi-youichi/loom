use async_trait::async_trait;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSource, ToolSourceError};
use crate::tools::{Tool, ToolRegistryLocked};

/// Aggregates multiple tools and implements ToolSource trait via ToolRegistry.
///
/// This is the bridge between the new Tool-based architecture and the existing
/// ToolSource trait. It internally uses ToolRegistryLocked to manage tools and
/// implements ToolSource by delegating to the registry.
///
/// # Examples
///
/// ```no_run
/// use graphweave::tools::{AggregateToolSource, Tool};
/// use graphweave::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec, ToolSource};
/// # use async_trait::async_trait;
/// # struct MockTool;
/// # #[async_trait] impl Tool for MockTool {
/// #     fn name(&self) -> &str { "mock" }
/// #     fn spec(&self) -> ToolSpec { todo!() }
/// #     async fn call(&self, _: serde_json::Value, _: Option<&ToolCallContext>) -> Result<ToolCallContent, ToolSourceError> { todo!() }
/// # }
/// # #[tokio::main]
/// # async fn main() {
/// use serde_json::json;
///
/// let source = AggregateToolSource::new();
/// source.register_sync(Box::new(MockTool));
///
/// let tools = source.list_tools().await.unwrap();
/// assert_eq!(tools.len(), 1);
///
/// let result = source.call_tool("mock", json!({})).await.unwrap();
/// # }
/// ```
///
/// # Interaction
///
/// - **ToolRegistryLocked**: Internal storage for tools
/// - **Tool**: Individual tools are registered here
/// - **ToolSource**: Implements this trait for use with ActNode and ThinkNode
/// - **ToolCallContext**: Context is passed through call_tool_with_context
pub struct AggregateToolSource {
    registry: ToolRegistryLocked,
    context: std::sync::Arc<std::sync::RwLock<Option<crate::tool_source::ToolCallContext>>>,
}

impl AggregateToolSource {
    /// Creates a new empty AggregateToolSource.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # #[tokio::main]
    /// # async fn main() {
    /// use graphweave::tools::AggregateToolSource;
    /// use graphweave::tool_source::ToolSource;
    ///
    /// let source = AggregateToolSource::new();
    /// let tools = source.list_tools().await.unwrap();
    /// assert_eq!(tools.len(), 0);
    /// # }
    /// ```
    pub fn new() -> Self {
        Self {
            registry: ToolRegistryLocked::new(),
            context: std::sync::Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Registers a tool with this source asynchronously.
    ///
    /// Prefer this when calling from async context (e.g. WebToolsSource::new) to avoid
    /// blocking the tokio worker.
    ///
    /// # Parameters
    ///
    /// - `tool`: Box<dyn Tool> to register
    pub async fn register_async(&self, tool: Box<dyn Tool>) {
        self.registry.register_async(tool).await;
    }

    /// Registers a tool with this source synchronously.
    ///
    /// Tools are stored in the internal ToolRegistryLocked and can be
    /// listed and called via ToolSource trait methods. Prefer [`register_async`](Self::register_async)
    /// when in async context.
    ///
    /// # Parameters
    ///
    /// - `tool`: Box<dyn Tool> to register
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use graphweave::tools::{AggregateToolSource, Tool};
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
    /// let source = AggregateToolSource::new();
    /// source.register_sync(Box::new(MockTool));
    /// # }
    /// ```
    pub fn register_sync(&self, tool: Box<dyn Tool>) {
        self.registry.register_sync(tool);
    }
}

impl Default for AggregateToolSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolSource for AggregateToolSource {
    /// Lists all registered tools.
    ///
    /// Delegates to ToolRegistryLocked::list() to get tool specifications.
    ///
    /// # Returns
    ///
    /// Vector of ToolSpec for all registered tools.
    ///
    /// # Errors
    ///
    /// Never fails (always returns Ok).
    ///
    /// # Interaction
    ///
    /// - Called by ThinkNode to build tool descriptions for LLM prompts
    /// - Delegates to ToolRegistryLocked::list()
    async fn list_tools(&self) -> Result<Vec<crate::tool_source::ToolSpec>, ToolSourceError> {
        Ok(self.registry.list().await)
    }

    /// Calls a tool by name with the given arguments.
    ///
    /// Delegates to ToolRegistryLocked::call() to execute the tool.
    ///
    /// # Parameters
    ///
    /// - `name`: Name of the tool to call
    /// - `arguments`: JSON arguments to pass to the tool
    ///
    /// # Returns
    ///
    /// ToolCallContent with the result of tool execution.
    ///
    /// # Errors
    ///
    /// Returns ToolSourceError::NotFound if tool name is not registered,
    /// or any error from the tool's call() method.
    ///
    /// # Interaction
    ///
    /// - Called by ActNode when executing tool calls
    /// - Delegates to ToolRegistryLocked::call()
    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let effective_ctx: Option<ToolCallContext> =
            self.context.read().ok().and_then(|g| g.as_ref().cloned());
        let effective_ctx_ref: Option<&ToolCallContext> = effective_ctx.as_ref();
        self.registry.call(name, arguments, effective_ctx_ref).await
    }

    /// Calls a tool by name with the given arguments and optional context.
    ///
    /// Delegates to ToolRegistryLocked::call() and passes through the context.
    /// This allows tools like GetRecentMessagesTool to access per-call state.
    ///
    /// # Parameters
    ///
    /// - `name`: Name of the tool to call
    /// - `arguments`: JSON arguments to pass to the tool
    /// - `ctx`: Optional context with recent messages and other per-call data
    ///
    /// # Returns
    ///
    /// ToolCallContent with the result of tool execution.
    ///
    /// # Errors
    ///
    /// Returns ToolSourceError::NotFound if tool name is not registered,
    /// or any error from the tool's call() method.
    ///
    /// # Interaction
    ///
    /// - Called by ActNode with ToolCallContext before executing tool calls
    /// - Context is set via set_call_context() before each round
    /// - Delegates to ToolRegistryLocked::call() with context
    async fn call_tool_with_context(
        &self,
        name: &str,
        arguments: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        if let Some(c) = ctx {
            if let Ok(mut g) = self.context.write() {
                *g = Some(c.clone());
            }
            self.registry.call(name, arguments, ctx).await
        } else {
            let effective_ctx: Option<ToolCallContext> =
                self.context.read().ok().and_then(|g| g.as_ref().cloned());
            let effective_ctx_ref: Option<&ToolCallContext> = effective_ctx.as_ref();
            self.registry.call(name, arguments, effective_ctx_ref).await
        }
    }

    fn set_call_context(&self, ctx: Option<ToolCallContext>) {
        if let Ok(mut g) = self.context.write() {
            *g = ctx;
        }
    }
}

#[async_trait]
impl ToolSource for std::sync::Arc<AggregateToolSource> {
    async fn list_tools(&self) -> Result<Vec<crate::tool_source::ToolSpec>, ToolSourceError> {
        self.as_ref().list_tools().await
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<ToolCallContent, ToolSourceError> {
        self.as_ref().call_tool(name, arguments).await
    }

    async fn call_tool_with_context(
        &self,
        name: &str,
        arguments: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        self.as_ref()
            .call_tool_with_context(name, arguments, ctx)
            .await
    }

    fn set_call_context(&self, ctx: Option<ToolCallContext>) {
        self.as_ref().set_call_context(ctx);
    }
}
