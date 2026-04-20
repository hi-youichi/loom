use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use loom::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use loom::tools::Tool;
use serde::Deserialize;
use serde_json::Value;

use super::{create_tool_spec, get_client_bridge, ClientBridgeTrait};

#[derive(Debug, Deserialize)]
struct CreateTerminalArgs {
    command: String,
    args: Option<Vec<String>>,
    cwd: Option<String>,
    env: Option<HashMap<String, String>>,
    output_byte_limit: Option<u64>,
}

pub struct CreateTerminalTool;

impl CreateTerminalTool {
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
            "Create a new terminal and execute a command. Returns a terminal_id for further operations (terminal_output, terminal_wait_for_exit, terminal_kill, terminal_release).",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The command to execute"
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Command arguments"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory (absolute path)"
                    },
                    "env": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "description": "Environment variables"
                    },
                    "output_byte_limit": {
                        "type": "integer",
                        "description": "Max output bytes to retain"
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

        let bridge: Arc<dyn ClientBridgeTrait> = get_client_bridge().await.map_err(|e| {
            ToolSourceError::Transport(format!("Failed to get client bridge: {}", e))
        })?;

        let env_pairs = args
            .env
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();

        let terminal_id = bridge
            .terminal_create(
                "default",
                &args.command,
                args.args.unwrap_or_default(),
                env_pairs,
                args.cwd,
                args.output_byte_limit,
            )
            .await
            .map_err(|e| ToolSourceError::Transport(format!("Terminal create failed: {}", e)))?;

        Ok(ToolCallContent::Terminal { terminal_id })
    }
}

#[derive(Debug, Deserialize)]
struct TerminalOutputArgs {
    terminal_id: String,
}

pub struct TerminalOutputTool;

impl TerminalOutputTool {
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
            "Get the current output of a terminal without waiting for completion. Returns output text, truncated flag, and exit status if the command has finished.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "terminal_id": {
                        "type": "string",
                        "description": "The terminal ID to get output from"
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

        let bridge: Arc<dyn ClientBridgeTrait> = get_client_bridge().await.map_err(|e| {
            ToolSourceError::Transport(format!("Failed to get client bridge: {}", e))
        })?;

        let output = bridge
            .terminal_output("default", &args.terminal_id)
            .await
            .map_err(|e| ToolSourceError::Transport(format!("Terminal output failed: {}", e)))?;

        let exit_status_json = output.exit_status.map(|s| {
            serde_json::json!({
                "exitCode": s.exit_code,
                "signal": s.signal
            })
        });

        let result = serde_json::json!({
            "output": output.output,
            "truncated": output.truncated,
            "exitStatus": exit_status_json,
        });

        Ok(ToolCallContent::Text(
            serde_json::to_string_pretty(&result)
                .unwrap_or_else(|_| "Terminal output retrieved".to_string()),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct WaitForExitArgs {
    terminal_id: String,
}

pub struct WaitForExitTool;

impl WaitForExitTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for WaitForExitTool {
    fn name(&self) -> &str {
        "terminal_wait_for_exit"
    }

    fn spec(&self) -> ToolSpec {
        create_tool_spec(
            "terminal_wait_for_exit",
            "Wait for a terminal command to complete. Returns exit code and signal.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "terminal_id": {
                        "type": "string",
                        "description": "The terminal ID to wait for"
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
        let args: WaitForExitArgs = serde_json::from_value(args)
            .map_err(|e| ToolSourceError::InvalidInput(format!("Invalid arguments: {}", e)))?;

        let bridge: Arc<dyn ClientBridgeTrait> = get_client_bridge().await.map_err(|e| {
            ToolSourceError::Transport(format!("Failed to get client bridge: {}", e))
        })?;

        let result = bridge
            .terminal_wait_for_exit("default", &args.terminal_id)
            .await
            .map_err(|e| {
                ToolSourceError::Transport(format!("Terminal wait_for_exit failed: {}", e))
            })?;

        let json = serde_json::json!({
            "exitCode": result.exit_code,
            "signal": result.signal
        });

        Ok(ToolCallContent::Text(
            serde_json::to_string_pretty(&json)
                .unwrap_or_else(|_| "Terminal exited".to_string()),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct KillTerminalArgs {
    terminal_id: String,
}

pub struct KillTerminalTool;

impl KillTerminalTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for KillTerminalTool {
    fn name(&self) -> &str {
        "terminal_kill"
    }

    fn spec(&self) -> ToolSpec {
        create_tool_spec(
            "terminal_kill",
            "Kill a running terminal command. The terminal remains valid for terminal_output and terminal_wait_for_exit. You must still call terminal_release when done.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "terminal_id": {
                        "type": "string",
                        "description": "The terminal ID to kill"
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
        let args: KillTerminalArgs = serde_json::from_value(args)
            .map_err(|e| ToolSourceError::InvalidInput(format!("Invalid arguments: {}", e)))?;

        let bridge: Arc<dyn ClientBridgeTrait> = get_client_bridge().await.map_err(|e| {
            ToolSourceError::Transport(format!("Failed to get client bridge: {}", e))
        })?;

        bridge
            .terminal_kill("default", &args.terminal_id)
            .await
            .map_err(|e| ToolSourceError::Transport(format!("Terminal kill failed: {}", e)))?;

        Ok(ToolCallContent::Text(format!(
            "Terminal {} killed",
            args.terminal_id
        )))
    }
}

#[derive(Debug, Deserialize)]
struct ReleaseTerminalArgs {
    terminal_id: String,
}

pub struct ReleaseTerminalTool;

impl ReleaseTerminalTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReleaseTerminalTool {
    fn name(&self) -> &str {
        "terminal_release"
    }

    fn spec(&self) -> ToolSpec {
        create_tool_spec(
            "terminal_release",
            "Release a terminal, killing it if still running and freeing all resources. After release the terminal_id is invalid for all other terminal methods.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "terminal_id": {
                        "type": "string",
                        "description": "The terminal ID to release"
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
        let args: ReleaseTerminalArgs = serde_json::from_value(args)
            .map_err(|e| ToolSourceError::InvalidInput(format!("Invalid arguments: {}", e)))?;

        let bridge: Arc<dyn ClientBridgeTrait> = get_client_bridge().await.map_err(|e| {
            ToolSourceError::Transport(format!("Failed to get client bridge: {}", e))
        })?;

        bridge
            .terminal_release("default", &args.terminal_id)
            .await
            .map_err(|e| ToolSourceError::Transport(format!("Terminal release failed: {}", e)))?;

        Ok(ToolCallContent::Text(format!(
            "Terminal {} released",
            args.terminal_id
        )))
    }
}

impl Default for CreateTerminalTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for TerminalOutputTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for WaitForExitTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for KillTerminalTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for ReleaseTerminalTool {
    fn default() -> Self {
        Self::new()
    }
}
