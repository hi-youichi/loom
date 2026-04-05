//! Terminal tools - Create and manage IDE terminals via ACP client.
//!
//! These tools delegate to the ACP client's terminal methods, which can
//! run commands in the IDE's integrated terminal.

use std::collections::HashMap;

use async_trait::async_trait;
use loom::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use loom::tools::Tool;
use serde::Deserialize;
use serde_json::Value;

use super::{create_tool_spec, get_client_bridge};

/// Arguments for terminal/create tool.
#[derive(Debug, Deserialize)]
struct CreateTerminalArgs {
    /// The command to execute (required).
    command: String,
    /// Optional arguments for the command.
    args: Option<Vec<String>>,
    /// Optional working directory for the terminal.
    cwd: Option<String>,
    /// Optional environment variables.
    env: Option<HashMap<String, String>>,
    /// Optional name/title for the terminal.
    name: Option<String>,
}

/// Tool to create an IDE terminal via ACP client.
pub struct CreateTerminalTool;

impl CreateTerminalTool {
    /// Create a new CreateTerminalTool.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CreateTerminalTool {
    fn name(&self) -> &str {
        "terminal_create"
    }

    fn spec(&self) -> ToolSpec {
        create_tool_spec(
            "terminal_create",
            "Create a new terminal in the IDE and execute a command. Returns a terminal ID that can be used \
             to get output and manage the terminal. The terminal runs in the IDE's integrated \
             terminal environment, providing better integration than external shell commands.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The command to execute"
                    },
                    "args": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Optional arguments for the command"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Optional working directory for the terminal"
                    },
                    "env": {
                        "type": "object",
                        "description": "Optional environment variables"
                    },
                    "name": {
                        "type": "string",
                        "description": "Optional name/title for the terminal tab"
                    }
                },
                "required": ["command"]
            }),
        )
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let args: CreateTerminalArgs = serde_json::from_value(args)
            .map_err(|e| ToolSourceError::InvalidInput(format!("Invalid arguments: {}", e)))?;

        // Check if bridge is available
        let bridge = get_client_bridge().await.map_err(|e| {
            ToolSourceError::Transport(format!("Failed to get client bridge: {}", e))
        })?;

        let terminal_id = bridge
            .create_terminal(
                &args.command,
                args.args.as_deref(),
                args.cwd.as_deref(),
                args.env.as_ref(),
                args.name.as_deref(),
            )
            .await
            .map_err(|e| ToolSourceError::Transport(format!("Failed to create terminal: {}", e)))?;

        let result = serde_json::json!({
            "terminal_id": terminal_id,
            "message": "Terminal created successfully"
        });

        Ok(ToolCallContent::text(
            serde_json::to_string_pretty(&result)
                .unwrap_or_else(|_| "Terminal created".to_string()),
        ))
    }
}

impl Default for CreateTerminalTool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_terminal_tool_spec() {
        let tool = CreateTerminalTool::new();
        let spec = tool.spec();
        assert_eq!(spec.name, "terminal_create");
        assert!(spec.description.is_some());
    }
}

/// Arguments for terminal/output tool.
#[derive(Debug, Deserialize)]
struct TerminalOutputArgs {
    /// The ID of the terminal to get output from.
    terminal_id: String,
}

/// Tool to get output from an IDE terminal via ACP client.
pub struct TerminalOutputTool;

impl TerminalOutputTool {
    /// Create a new TerminalOutputTool.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TerminalOutputTool {
    fn name(&self) -> &str {
        "terminal_output"
    }

    fn spec(&self) -> ToolSpec {
        create_tool_spec(
            "terminal_output",
            "Get the output from a terminal created by terminal_create. \
             Returns the output text, whether it was truncated, and optionally the exit status \
             if the command has completed.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "terminal_id": {
                        "type": "string",
                        "description": "The ID of the terminal to get output from"
                    }
                },
                "required": ["terminal_id"]
            }),
        )
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let args: TerminalOutputArgs = serde_json::from_value(args)
            .map_err(|e| ToolSourceError::InvalidInput(format!("Invalid arguments: {}", e)))?;

        let bridge = get_client_bridge().await.map_err(|e| {
            ToolSourceError::Transport(format!("Failed to get client bridge: {}", e))
        })?;

        let output = bridge
            .terminal_output(&args.terminal_id)
            .await
            .map_err(|e| {
                ToolSourceError::Transport(format!("Failed to get terminal output: {}", e))
            })?;

        let result = serde_json::json!({
            "output": output.output,
            "truncated": output.truncated,
            "exit_status": output.exit_status,
        });

        Ok(ToolCallContent::text(
            serde_json::to_string_pretty(&result)
                .unwrap_or_else(|_| "Terminal output retrieved".to_string()),
        ))
    }
}

impl Default for TerminalOutputTool {
    fn default() -> Self {
        Self::new()
    }
}
