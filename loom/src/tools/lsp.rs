//! LSP tool (experimental): placeholder for LSP-based completions/diagnostics.
//!
//! Not implemented in this build; returns a message directing the agent to use other tools.

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

/// Tool name for LSP (experimental).
pub const TOOL_LSP: &str = "lsp";

/// Placeholder LSP tool. Returns a message that LSP is not available.
pub struct LspTool;

impl LspTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LspTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        TOOL_LSP
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_LSP.to_string(),
            description: Some(
                "(Experimental) LSP-based code completions and diagnostics. \
                 Currently not implemented; use read, edit, and grep for code navigation and edits."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "filePath": { "type": "string", "description": "Path to the file." },
                    "position": { "type": "object", "description": "Optional line/character." }
                }
            }),
        }
    }

    async fn call(
        &self,
        _args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        Ok(ToolCallContent {
            text: "LSP tool is experimental and not implemented in this build. Use read, edit, grep, and glob for code navigation and edits.".to_string(),
        })
    }
}
