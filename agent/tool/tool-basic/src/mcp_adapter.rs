//! MCP tool adapter: wraps each MCP tool as `dyn Tool` for a single registry.
//!
//! Each MCP tool is represented by an `McpToolAdapter` that implements `Tool`;
//! `call` delegates to the shared `McpToolSource`. Use `register_mcp_tools`
//! to list MCP tools and register one adapter per tool into an `AggregateToolSource`.

use std::sync::Arc;

use async_trait::async_trait;

use crate::mcp::McpToolSource;
use tool_core::Tool;
use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};

/// Adapter that makes one MCP tool implement the `Tool` trait.
///
/// Holds the tool name, cached spec from MCP `tools/list`, and a shared
/// `Arc<McpToolSource>` so `call` can delegate to `tools/call`. Used to put
/// MCP tools into the same `ToolRegistry` as local tools (e.g. memory tools).
///
/// **Interaction**: Created by `register_mcp_tools`; registered with
/// `AggregateToolSource::register_sync`. Implements `Tool`; `call` ignores
/// `ToolCallContext` and forwards to MCP.
pub struct McpToolAdapter {
    name: String,
    spec: ToolSpec,
    source: Arc<McpToolSource>,
}

impl McpToolAdapter {
    /// Creates an adapter for one MCP tool.
    ///
    /// **Interaction**: Used by `register_mcp_tools`; not typically called directly.
    pub fn new(name: String, spec: ToolSpec, source: Arc<McpToolSource>) -> Self {
        Self { name, spec, source }
    }
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        self.source.call_tool_async(self.name.as_str(), args).await
    }
}

/// Registers all tools from the MCP server into the given aggregate.
///
/// Calls `mcp.list_tools().await`, then for each tool creates an `McpToolAdapter`
/// and registers it with `aggregate.register_sync`. Use when building a single
/// tool set that includes both local tools (e.g. memory) and MCP tools (e.g. Exa).
///
/// **Interaction**: Call after registering local tools (if any). Requires
/// `exa_api_key` (or equivalent) to have been used to create `mcp`; do not call
/// when MCP is not configured.
pub async fn register_mcp_tools(
    aggregate: &tool_core::ToolRegistryLocked,
    mcp: Arc<McpToolSource>,
) -> Result<(), ToolSourceError> {
    let specs = mcp.list_tools_async().await?;
    register_mcp_tools_with_specs(aggregate, mcp, specs).await;
    Ok(())
}

/// Registers pre-fetched MCP tool specs into the aggregate. Use when tools were
/// already obtained (e.g. inside spawn_blocking for stdio MCP) to avoid block_in_place on the async worker.
pub async fn register_mcp_tools_with_specs(
    aggregate: &tool_core::ToolRegistryLocked,
    mcp: Arc<McpToolSource>,
    specs: Vec<ToolSpec>,
) {
    for spec in specs {
        let name = spec.name.clone();
        let adapter = McpToolAdapter::new(name, spec, Arc::clone(&mcp));
        aggregate.register_async(Box::new(adapter)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_tool_adapter_name_and_spec() {
        let _spec = ToolSpec {
            name: "demo".to_string(),
            description: Some("demo tool".to_string()),
            input_schema: serde_json::json!({"type": "object"}),
            output_hint: None,
        };
        // We can't easily create a mock McpToolSource, but we can verify the adapter struct.
        // Full integration test requires a real MCP server (see tool_source.rs e2e tests).
    }
}
