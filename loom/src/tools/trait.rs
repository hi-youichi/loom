use async_trait::async_trait;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};

/// Represents a single tool that can be called by the LLM.
///
/// Each tool has a unique name, a specification (description and JSON schema),
/// and implements the call logic. Tools are registered with ToolRegistry and
/// can be called via AggregateToolSource.
///
/// # Examples
///
/// ```
/// use async_trait::async_trait;
/// use serde_json::Value;
/// use loom::tools::Tool;
/// use loom::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
///
/// struct MyTool;
///
/// #[async_trait]
/// impl Tool for MyTool {
///     fn name(&self) -> &str {
///         "my_tool"
///     }
///
///     fn spec(&self) -> ToolSpec {
///         ToolSpec {
///             name: "my_tool".to_string(),
///             description: Some("A sample tool".to_string()),
///             input_schema: serde_json::json!({}),
///         }
///     }
///
///     async fn call(
///         &self,
///         args: Value,
///         _ctx: Option<&ToolCallContext>,
///     ) -> Result<ToolCallContent, ToolSourceError> {
///         Ok(ToolCallContent {
///             text: "tool executed".to_string(),
///         })
///     }
/// }
/// ```
///
/// # Interaction
///
/// - **ToolRegistry**: Stores tools by name in a HashMap
/// - **AggregateToolSource**: Implements ToolSource trait by delegating to ToolRegistry
/// - **ToolCallContext**: Optional per-call context (e.g., recent messages) passed during call
#[async_trait]
pub trait Tool: Send + Sync {
    /// Returns the unique name of this tool.
    ///
    /// Must be unique across all tools registered in a ToolRegistry.
    /// This name is used to identify the tool when calling it.
    fn name(&self) -> &str;

    /// Returns the specification for this tool.
    ///
    /// Includes tool name, description (for the LLM), and JSON schema for arguments.
    /// The spec is used by ThinkNode to build prompts and validate tool calls.
    ///
    /// # Interaction
    ///
    /// - Called by ToolRegistry::list() to build Vec<ToolSpec>
    /// - Spec fields are aligned with MCP tools/list result
    fn spec(&self) -> ToolSpec;

    /// Executes the tool with the given arguments and optional context.
    ///
    /// # Parameters
    ///
    /// - `args`: JSON value containing tool arguments (validated against input_schema)
    /// - `ctx`: Optional per-call context with recent messages (used by conversation tools)
    ///
    /// # Returns
    ///
    /// Tool execution result as text content.
    ///
    /// # Errors
    ///
    /// Returns ToolSourceError for:
    /// - Invalid arguments (validation failure)
    /// - Execution errors (e.g., store errors)
    /// - Transport errors (e.g., network issues)
    ///
    /// # Interaction
    ///
    /// - Called by ToolRegistry::call() which validates tool name exists
    /// - ToolRegistryLocked wraps ToolRegistry with async RwLock for thread safety
    /// - Context is provided by ActNode via ToolCallContext before tool calls
    async fn call(
        &self,
        args: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError>;
}
