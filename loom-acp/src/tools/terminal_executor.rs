use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use loom::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use loom::tools::CommandExecutor;
use tracing::{debug, error, info, warn};

use crate::terminal::TerminalManager;

pub struct TerminalCommandExecutor {
    terminal_mgr: Arc<TerminalManager>,
}

impl TerminalCommandExecutor {
    pub fn new(terminal_mgr: Arc<TerminalManager>) -> Self {
        Self { terminal_mgr }
    }
}

#[async_trait]
impl CommandExecutor for TerminalCommandExecutor {
    async fn execute(
        &self,
        command: &str,
        working_dir: Option<&Path>,
        timeout_ms: Option<u64>,
        _env: Vec<(String, String)>,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let (shell, args) = if cfg!(windows) {
            let wrapped = format!("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {}", command);
            ("powershell".to_string(), vec!["-NoProfile".to_string(), "-Command".to_string(), wrapped])
        } else {
            ("sh".to_string(), vec!["-c".to_string(), command.to_string()])
        };

        info!(
            executor = "local",
            command = %command,
            shell = %shell,
            working_dir = ?working_dir,
            timeout_ms = ?timeout_ms,
            env_count = _env.len(),
            "bash execute called"
        );

        let terminal_id = self
            .terminal_mgr
            .create_terminal(
                shell,
                args,
                working_dir.map(|p| p.to_path_buf()),
                _env,
                None,
            )
            .await
            .map_err(|e| {
                error!(command = %command, error = %e, "bash execute: terminal create failed");
                ToolSourceError::Transport(e.to_string())
            })?;

        debug!(terminal_id = %terminal_id, "bash execute: terminal created");

        let result = if let Some(timeout) = timeout_ms {
            debug!(terminal_id = %terminal_id, timeout_ms = timeout, "bash execute: waiting with timeout");
            tokio::select! {
                status = self.terminal_mgr.wait_for_exit(&terminal_id) => {
                    debug!(terminal_id = %terminal_id, "bash execute: process exited before timeout");
                    status.map_err(|e| {
                        error!(terminal_id = %terminal_id, error = %e, "bash execute: wait_for_exit failed");
                        ToolSourceError::Transport(e.to_string())
                    })
                }
                _ = tokio::time::sleep(Duration::from_millis(timeout)) => {
                    warn!(terminal_id = %terminal_id, timeout_ms = timeout, "bash execute: command timed out");
                    self.terminal_mgr.kill(&terminal_id).await.ok();
                    Err(ToolSourceError::Transport("Command timed out".into()))
                }
            }
        } else {
            debug!(terminal_id = %terminal_id, "bash execute: waiting for exit");
            self.terminal_mgr
                .wait_for_exit(&terminal_id)
                .await
                .map_err(|e| {
                    error!(terminal_id = %terminal_id, error = %e, "bash execute: wait_for_exit failed");
                    ToolSourceError::Transport(e.to_string())
                })
        };

        let _ = result;

        let (output, _truncated, _status) = self
            .terminal_mgr
            .get_output(&terminal_id)
            .await
            .unwrap_or_default();

        debug!(
            terminal_id = %terminal_id,
            output_len = output.len(),
            truncated = _truncated,
            "bash execute: output retrieved"
        );

        let _ = self.terminal_mgr.release(&terminal_id).await;
        debug!(terminal_id = %terminal_id, "bash execute: terminal released");

        if output.is_empty() {
            info!(terminal_id = %terminal_id, output_len = 0, "bash execute completed");
            Ok(ToolCallContent::text("(no output)"))
        } else {
            info!(terminal_id = %terminal_id, output_len = output.len(), "bash execute completed");
            Ok(ToolCallContent::text(output))
        }
    }
}

pub struct AcpBridgeCommandExecutor;

impl Default for AcpBridgeCommandExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl AcpBridgeCommandExecutor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CommandExecutor for AcpBridgeCommandExecutor {
    async fn execute(
        &self,
        command: &str,
        working_dir: Option<&Path>,
        timeout_ms: Option<u64>,
        env: Vec<(String, String)>,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let session_id = ctx
            .and_then(|c| c.acp_session_id.as_deref())
            .unwrap_or("default");

        if session_id == "default" {
            warn!(
                "bash execute: using fallback session_id='default'. \
                 acp_session_id not set in ToolCallContext — check RunOptions propagation chain"
            );
        }

        let (shell, args) = if cfg!(windows) {
            let wrapped = format!("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {}", command);
            ("powershell".to_string(), vec!["-NoProfile".to_string(), "-Command".to_string(), wrapped])
        } else {
            ("sh".to_string(), vec!["-c".to_string(), command.to_string()])
        };

        info!(
            executor = "acp_bridge",
            command = %command,
            session_id = %session_id,
            shell = %shell,
            working_dir = ?working_dir,
            timeout_ms = ?timeout_ms,
            env_count = env.len(),
            "bash execute called"
        );

        let bridge = crate::tools::get_client_bridge()
            .await
            .map_err(|e| {
                error!(command = %command, error = %e, "bash execute: failed to get client bridge");
                ToolSourceError::Transport(e)
            })?;

        let cwd = working_dir.map(|p| p.display().to_string());

        let terminal_id = bridge
            .terminal_create(session_id, &shell, args, env, cwd, None)
            .await
            .map_err(|e| {
                error!(command = %command, session_id = %session_id, error = %e, "bash execute: terminal create failed");
                ToolSourceError::Transport(e)
            })?;

        debug!(terminal_id = %terminal_id, "bash execute: terminal created via bridge");

        if let Some(timeout) = timeout_ms {
            debug!(terminal_id = %terminal_id, timeout_ms = timeout, "bash execute: waiting with timeout");
            tokio::select! {
                result = bridge.terminal_wait_for_exit(session_id, &terminal_id) => {
                    debug!(terminal_id = %terminal_id, "bash execute: process exited before timeout");
                    let _ = result;
                }
                _ = tokio::time::sleep(Duration::from_millis(timeout)) => {
                    warn!(terminal_id = %terminal_id, timeout_ms = timeout, "bash execute: command timed out, killing");
                    let _ = bridge.terminal_kill(session_id, &terminal_id).await;
                    let _ = bridge.terminal_release(session_id, &terminal_id).await;
                    return Err(ToolSourceError::Transport("Command timed out".into()));
                }
            }
        } else {
            debug!(terminal_id = %terminal_id, "bash execute: waiting for exit");
            let _ = bridge
                .terminal_wait_for_exit(session_id, &terminal_id)
                .await
                .map_err(|e| {
                    error!(terminal_id = %terminal_id, error = %e, "bash execute: wait_for_exit failed");
                    ToolSourceError::Transport(format!("terminal wait: {}", e))
                })?;
        }

        let output = bridge
            .terminal_output(session_id, &terminal_id)
            .await
            .map_err(|e| {
                error!(terminal_id = %terminal_id, error = %e, "bash execute: terminal_output failed");
                ToolSourceError::Transport(format!("terminal output: {}", e))
            })?;

        let _ = bridge.terminal_release(session_id, &terminal_id).await;
        debug!(terminal_id = %terminal_id, output_len = output.output.len(), "bash execute: terminal released");

        if output.output.is_empty() {
            info!(terminal_id = %terminal_id, output_len = 0, "bash execute completed");
            Ok(ToolCallContent::text("(no output)"))
        } else {
            info!(terminal_id = %terminal_id, output_len = output.output.len(), "bash execute completed");
            Ok(ToolCallContent::text(output.output))
        }
    }
}
