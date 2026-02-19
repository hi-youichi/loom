//! Tool source abstraction: list tools and call a tool.
//!
//! ReAct/Agent depends on `ToolSource` instead of a concrete tool registry;
//! implementations include `MockToolSource` (tests), `StoreToolSource`, `ShortTermMemoryToolSource`, `WebToolsSource`, and `McpToolSource` (feature mcp).
//!
//! ## Memory tools
//!
//! - **StoreToolSource**: long-term memory as tools (`remember`, `recall`, `search_memories`, `list_memories`).
//!   Use with `Arc<dyn Store>` and a fixed namespace; pass to `ActNode::new(Box::new(store_tools))`.
//! - **ShortTermMemoryToolSource**: one optional tool `get_recent_messages` (current conversation).
//!   Use only when you need to explicitly re-read or summarize last N messages; most flows can omit it.
//!   ActNode passes `ToolCallContext` via `call_tool_with_context` so this tool receives `state.messages`.
//! - **MemoryToolsSource**: composite of both. Use `MemoryToolsSource::new(store, namespace)` and pass to `ActNode::new(Box::new(memory_tools))` for one-line setup.
//!
//! ## Web tools
//!
//! - **WebToolsSource**: web fetching as tool (`web_fetcher`).
//!   Use `WebToolsSource::new()` to enable HTTP GET/POST capabilities; pass to `ActNode::new(Box::new(web_tools))`.
//! - **BashToolsSource**: shell command execution as tool (`bash`).
//!   Use `BashToolsSource::new()` to enable running shell commands; pass to `ActNode::new(Box::new(bash_tools))`.

mod bash_tools_source;
mod context;
mod file_tool_source;
mod memory_tools_source;
mod mock;
mod read_only_dir_tool_source;
mod short_term_memory_tool_source;
mod store_tool_source;
mod web_tools_source;
mod yaml_specs;

mod mcp;

pub use bash_tools_source::{BashToolsSource, TOOL_BASH};
pub use context::ToolCallContext;
pub use file_tool_source::{register_file_tools, FileToolSource};
pub use memory_tools_source::MemoryToolsSource;
pub use mock::MockToolSource;
pub use read_only_dir_tool_source::{
    register_read_only_dir_tools, ReadOnlyDirToolSource, TOOL_READ_ONLY_LIST_DIR,
    TOOL_READ_ONLY_READ_FILE,
};
pub use short_term_memory_tool_source::{ShortTermMemoryToolSource, TOOL_GET_RECENT_MESSAGES};
pub use store_tool_source::{
    StoreToolSource, TOOL_LIST_MEMORIES, TOOL_RECALL, TOOL_REMEMBER, TOOL_SEARCH_MEMORIES,
};
pub use web_tools_source::{WebToolsSource, TOOL_WEB_FETCHER};
pub use yaml_specs::{load_tool_specs, YamlSpecError, YamlSpecToolSource};

pub use mcp::{McpSession, McpSessionError, McpToolSource};

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

/// Tool specification, aligned with MCP `tools/list` result item.
///
/// Used by ReAct/Think to build tool descriptions for the LLM.
/// Supports deserialization from YAML for tool definitions.
///
/// **Interaction**: Returned by `ToolSource::list_tools()`; consumed by ThinkNode
/// to build prompts (future).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolSpec {
    /// Tool name (e.g. used in MCP tools/call).
    pub name: String,
    /// Human-readable description for the LLM.
    pub description: Option<String>,
    /// JSON Schema for arguments (MCP inputSchema).
    pub input_schema: Value,
}

/// Result of a single tool call; aligns with MCP `tools/call` content.
///
/// **Interaction**: Returned by `ToolSource::call_tool()`; ActNode maps this to
/// `ToolResult` and writes into `ReActState::tool_results`.
#[derive(Debug, Clone)]
pub struct ToolCallContent {
    /// Result text (e.g. from MCP result.content[].text).
    pub text: String,
}

/// Errors from listing or calling tools (ToolSource or MCP).
///
/// **Interaction**: Returned by `ToolSource::list_tools()` and `call_tool()`;
/// nodes may map to `AgentError` when running the graph.
#[derive(Debug, Error)]
pub enum ToolSourceError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("invalid arguments: {0}")]
    InvalidInput(String),
    #[error("MCP/transport error: {0}")]
    Transport(String),
    #[error("JSON-RPC error: {0}")]
    JsonRpc(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Scenario**: Display of each ToolSourceError variant contains expected keywords.
    #[test]
    fn tool_source_error_display_all_variants() {
        let s = ToolSourceError::NotFound("x".into()).to_string();
        assert!(s.to_lowercase().contains("not found"), "{}", s);
        let s = ToolSourceError::InvalidInput("bad".into()).to_string();
        assert!(s.to_lowercase().contains("invalid"), "{}", s);
        let s = ToolSourceError::Transport("net".into()).to_string();
        assert!(
            s.to_lowercase().contains("transport") || s.to_lowercase().contains("mcp"),
            "{}",
            s
        );
        let s = ToolSourceError::JsonRpc("rpc".into()).to_string();
        assert!(
            s.to_lowercase().contains("json") || s.to_lowercase().contains("rpc"),
            "{}",
            s
        );
    }

    /// **Scenario**: ToolSpec and ToolCallContent can be constructed and cloned.
    #[test]
    fn tool_spec_and_tool_call_content_construct_and_clone() {
        let spec = ToolSpec {
            name: "get_time".into(),
            description: Some("Get time".into()),
            input_schema: serde_json::json!({}),
        };
        assert_eq!(spec.name, "get_time");
        let _ = spec.clone();
        let content = ToolCallContent {
            text: "12:00".into(),
        };
        assert_eq!(content.text, "12:00");
        let _ = content.clone();
    }
}

/// Tool source: list tools and call a tool.
///
/// ReAct/Agent depends on this instead of a concrete ToolRegistry. Think node
/// uses `list_tools()` to build prompts; Act node uses `call_tool(name, args)`.
/// Implementations: `MockToolSource` (tests), `StoreToolSource`, `ShortTermMemoryToolSource`, `McpToolSource`.
///
/// **Call context**: Tools that need current-step state (e.g. recent messages) receive
/// it via `set_call_context`; ActNode calls it before each round of tool execution.
/// Default implementation is no-op.
///
/// **Interaction**: Used by ThinkNode (list_tools) and ActNode (call_tool, set_call_context).
#[async_trait]
pub trait ToolSource: Send + Sync {
    /// List available tools (e.g. MCP tools/list).
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError>;

    /// Call a tool by name with JSON arguments (e.g. MCP tools/call).
    async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<ToolCallContent, ToolSourceError>;

    /// Call a tool with optional per-step context (e.g. current messages).
    /// Default implementation ignores `ctx` and calls `call_tool(name, arguments)`.
    /// Implementations that need context (e.g. ShortTermMemoryToolSource for get_recent_messages)
    /// override and use `ctx.recent_messages`. ActNode calls this with `Some(&ToolCallContext)`
    /// so context is explicit and no cross-call state is needed.
    async fn call_tool_with_context(
        &self,
        name: &str,
        arguments: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let _ = ctx;
        self.call_tool(name, arguments).await
    }

    /// Injects per-step context before tool calls (e.g. current messages).
    /// ActNode calls this before executing tool_calls; implementations that need
    /// context (e.g. ShortTermMemoryToolSource) override; others use this default no-op.
    fn set_call_context(&self, _ctx: Option<ToolCallContext>) {}
}
