//! MCP session: stdio transport using rmcp client.
//!
//! Spawns MCP server process, performs initialize handshake automatically via rmcp,
//! provides `list_tools` and `call_tool` for JSON-RPC calls.

use rmcp::{transport::TokioChildProcess, ServiceExt};
use serde_json::Value;
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::Mutex;

use tool_core::ToolSourceError;

/// MCP session over stdio: spawns server process, performs initialize handshake
/// via rmcp, provides `list_tools` and `call_tool`.
pub struct McpSession {
    client: Mutex<Option<rmcp::service::RunningService<rmcp::service::RoleClient, ()>>>,
}

#[cfg(target_os = "windows")]
fn wrap_cmd_for_windows(command: String, args: Vec<String>) -> (String, Vec<String>) {
    let needs_wrap = command.eq_ignore_ascii_case("npx")
        || command.eq_ignore_ascii_case("npm")
        || command.eq_ignore_ascii_case("yarn")
        || command.eq_ignore_ascii_case("pnpm")
        || command.to_ascii_lowercase().ends_with(".cmd")
        || command.to_ascii_lowercase().ends_with(".bat");
    if needs_wrap {
        let mut wrapped_args = vec!["/C".to_string(), command];
        wrapped_args.extend(args);
        ("cmd".to_string(), wrapped_args)
    } else {
        (command, args)
    }
}

#[cfg(not(target_os = "windows"))]
fn wrap_cmd_for_windows(command: String, args: Vec<String>) -> (String, Vec<String>) {
    (command, args)
}

impl McpSession {
    /// Creates a new MCP session by spawning the server process and completing
    /// the initialize handshake via rmcp. Returns `Err` if spawn or initialize fails.
    pub async fn new(
        command: impl Into<String>,
        args: Vec<String>,
        env: Option<impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>>,
        stderr_verbose: bool,
    ) -> Result<Self, McpSessionError> {
        let (command, args) = wrap_cmd_for_windows(command.into(), args);
        let mut cmd = Command::new(command);
        cmd.args(args);
        if let Some(env_iter) = env {
            for (k, v) in env_iter {
                cmd.env(k.into(), v.into());
            }
        }
        // MCP stdio servers are background processes. On Windows, explicitly
        // suppress the console window even when the configured command is
        // wrapped through `cmd /C` (for example `npx`, `npm`, or a `.cmd` file).
        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        if !stderr_verbose {
            cmd.stderr(Stdio::null());
        }

        let transport =
            TokioChildProcess::new(cmd).map_err(|e| McpSessionError::Transport(e.to_string()))?;
        let client =
            ().serve(transport)
                .await
                .map_err(|e| McpSessionError::Initialize(e.to_string()))?;
        Ok(Self {
            client: Mutex::new(Some(client)),
        })
    }

    /// Gracefully closes the MCP client and its child process.
    pub async fn shutdown(&self) {
        let client = self.client.lock().await.take();
        if let Some(client) = client {
            if let Err(error) = client.cancel().await {
                tracing::debug!(%error, "MCP client shutdown failed");
            }
        }
    }

    pub async fn is_closed(&self) -> bool {
        self.client
            .lock()
            .await
            .as_ref()
            .map(|client| client.is_closed())
            .unwrap_or(true)
    }

    /// Lists tools from the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<rmcp::model::Tool>, ToolSourceError> {
        self.client
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| ToolSourceError::Transport("MCP session is closed".into()))?
            .list_tools(Default::default())
            .await
            .map(|r| r.tools)
            .map_err(|e| ToolSourceError::Transport(e.to_string()))
    }

    /// Calls a tool on the MCP server.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<rmcp::model::CallToolResult, ToolSourceError> {
        let arguments = arguments.as_object().cloned();
        self.client
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| ToolSourceError::Transport("MCP session is closed".into()))?
            .call_tool(rmcp::model::CallToolRequestParams {
                name: name.to_string().into(),
                arguments,
                meta: Default::default(),
                task: None,
            })
            .await
            .map_err(|e| ToolSourceError::Transport(e.to_string()))
    }
}

/// Errors from McpSession operations.
#[derive(Debug, thiserror::Error)]
pub enum McpSessionError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("initialize: {0}")]
    Initialize(String),
}
