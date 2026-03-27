//! File system tools via ACP client.
//!
//! These tools delegate file operations to the IDE via ACP protocol,
//! allowing access to unsaved buffer contents and IDE workspace files.

use async_trait::async_trait;
use loom::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use loom::tools::Tool;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use super::{create_tool_spec, get_client_bridge, ClientBridgeTrait};

// ============================================================================
// ReadTextFile Tool
// ============================================================================

/// Arguments for fs/read_text_file tool.
#[derive(Debug, Deserialize)]
struct ReadTextFileArgs {
    /// Path to the file to read (relative to workspace root or absolute).
    path: String,
    /// Line number to start reading from (1-based, optional).
    line: Option<u32>,
    /// Maximum number of lines to read (optional).
    limit: Option<u32>,
}

/// Tool to read text files via ACP client.
///
/// This tool uses the ACP client's `read_text_file` method, which can access
/// files in the IDE's workspace, including unsaved buffer contents that haven't
/// been written to disk yet.
pub struct ReadTextFileTool;

impl ReadTextFileTool {
    /// Create a new ReadTextFileTool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReadTextFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ReadTextFileTool {
    fn name(&self) -> &str {
        "fs/read_text_file"
    }

    fn spec(&self) -> ToolSpec {
        create_tool_spec(
            "fs/read_text_file",
            "Read a text file from the IDE's workspace. This can access files that are \
             open in the IDE with unsaved changes, providing access to the current buffer \
             content rather than what's on disk. Use this instead of the local filesystem \
             when you need to see what the user is currently editing. Supports pagination \
             via 'line' and 'limit' parameters for reading large files.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read (relative to workspace root or absolute)"
                    },
                    "line": {
                        "type": "integer",
                        "description": "Line number to start reading from (1-based)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to read"
                    }
                },
                "required": ["path"]
            }),
        )
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let args: ReadTextFileArgs = serde_json::from_value(args)
            .map_err(|e| ToolSourceError::InvalidInput(format!("Invalid arguments: {}", e)))?;

        let bridge: Arc<dyn ClientBridgeTrait> = get_client_bridge()
            .map_err(|e| ToolSourceError::Transport(format!("Failed to get client bridge: {}", e)))?;

        let content = bridge
            .read_text_file(&args.path, args.line, args.limit)
            .await
            .map_err(|e| ToolSourceError::Transport(format!("Failed to read file: {}", e)))?;

        Ok(ToolCallContent { text: content })
    }
}

// ============================================================================
// WriteTextFile Tool
// ============================================================================

/// Arguments for fs/write_text_file tool.
#[derive(Debug, Deserialize)]
struct WriteTextFileArgs {
    /// Path to the file to write (relative to workspace root or absolute).
    path: String,
    /// Content to write to the file.
    content: String,
}

/// Tool to write text files via ACP client.
///
/// This tool uses the ACP client's `write_text_file` method, which can write
/// files in the IDE's workspace. The IDE may prompt the user for confirmation
/// before actually writing the file.
pub struct WriteTextFileTool;

impl WriteTextFileTool {
    /// Create a new WriteTextFileTool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for WriteTextFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WriteTextFileTool {
    fn name(&self) -> &str {
        "fs/write_text_file"
    }

    fn spec(&self) -> ToolSpec {
        create_tool_spec(
            "fs/write_text_file",
            "Write a text file to the IDE's workspace. The IDE may show the file as unsaved \
             or prompt for confirmation. Use this when you need to create or modify files \
             in the user's workspace.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to write (relative to workspace root or absolute)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }),
        )
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let args: WriteTextFileArgs = serde_json::from_value(args)
            .map_err(|e| ToolSourceError::InvalidInput(format!("Invalid arguments: {}", e)))?;

        // Get the global bridge
        let bridge: Arc<dyn ClientBridgeTrait> = get_client_bridge()
            .map_err(|e| ToolSourceError::Transport(format!("Failed to get client bridge: {}", e)))?;

        bridge
            .write_text_file(&args.path, &args.content)
            .await
            .map_err(|e| ToolSourceError::Transport(format!("Failed to write file: {}", e)))?;

        Ok(ToolCallContent {
            text: format!("Successfully wrote to '{}'", args.path),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_text_file_spec() {
        let tool = ReadTextFileTool::new();
        let spec = tool.spec();
        assert_eq!(spec.name, "fs/read_text_file");
        assert!(spec.description.is_some());
    }

    #[test]
    fn test_write_text_file_spec() {
        let tool = WriteTextFileTool::new();
        let spec = tool.spec();
        assert_eq!(spec.name, "fs/write_text_file");
        assert!(spec.description.is_some());
    }
}
