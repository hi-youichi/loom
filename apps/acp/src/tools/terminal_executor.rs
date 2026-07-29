use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tool_basic::bash::CommandExecutor;
use tool_basic::shared::shell_output::format_terminal_timed_out_output;
use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};
use tracing::{error, info, instrument, warn};

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
    #[instrument(skip_all, fields(command, working_dir, timeout_ms, executor = "local"))]
    async fn execute(
        &self,
        command: &str,
        working_dir: Option<&Path>,
        timeout_ms: Option<u64>,
        _env: Vec<(String, String)>,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let (shell, args) = if cfg!(windows) {
            let wrapped = format!(
                "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {}",
                command
            );
            (
                "powershell".to_string(),
                vec!["-NoProfile".to_string(), "-Command".to_string(), wrapped],
            )
        } else {
            (
                "sh".to_string(),
                vec!["-c".to_string(), command.to_string()],
            )
        };

        info!(
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
                error!(error = %e, "terminal create failed");
                ToolSourceError::Transport(e.to_string())
            })?;

        info!(terminal_id = %terminal_id, "terminal created");

        if let Some(timeout) = timeout_ms {
            info!(terminal_id = %terminal_id, timeout_ms = timeout, "waiting for exit with timeout");
            tokio::select! {
                status = self.terminal_mgr.wait_for_exit(&terminal_id) => {
                    info!(terminal_id = %terminal_id, "process exited before timeout");
                    let _exit_status = status.map_err(|e| {
                        error!(terminal_id = %terminal_id, error = %e, "wait_for_exit failed");
                        ToolSourceError::Transport(e.to_string())
                    })?;

                    let (output, _truncated, _status) = self
                        .terminal_mgr
                        .get_output(&terminal_id)
                        .await
                        .unwrap_or_default();

                    info!(
                        terminal_id = %terminal_id,
                        output_len = output.len(),
                        "output retrieved"
                    );

                    let _ = self.terminal_mgr.release(&terminal_id).await;
                    info!(terminal_id = %terminal_id, "terminal released");

                    if output.is_empty() {
                        Ok(ToolCallContent::text("(no output)"))
                    } else {
                        Ok(ToolCallContent::text(output))
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(timeout)) => {
                    warn!(terminal_id = %terminal_id, timeout_ms = timeout, "command timed out, detaching terminal");

                    let (output, _truncated, _status) = self
                        .terminal_mgr
                        .get_output(&terminal_id)
                        .await
                        .unwrap_or_default();

                    let text = format_terminal_timed_out_output(&terminal_id, &output);

                    info!(
                        terminal_id = %terminal_id,
                        output_len = output.len(),
                        "terminal detached, partial output returned"
                    );

                    Ok(ToolCallContent::text(text))
                }
            }
        } else {
            info!(terminal_id = %terminal_id, "waiting for exit (no timeout)");
            let exit_status = self
                .terminal_mgr
                .wait_for_exit(&terminal_id)
                .await
                .map_err(|e| {
                    error!(terminal_id = %terminal_id, error = %e, "wait_for_exit failed");
                    ToolSourceError::Transport(e.to_string())
                })?;

            info!(terminal_id = %terminal_id, exit_status = ?exit_status, "wait_for_exit completed");

            let (output, _truncated, _status) = self
                .terminal_mgr
                .get_output(&terminal_id)
                .await
                .unwrap_or_default();

            info!(
                terminal_id = %terminal_id,
                output_len = output.len(),
                truncated = _truncated,
                "output retrieved"
            );

            let _ = self.terminal_mgr.release(&terminal_id).await;
            info!(terminal_id = %terminal_id, "terminal released");

            if output.is_empty() {
                info!(terminal_id = %terminal_id, output_len = 0, "bash execute completed");
                Ok(ToolCallContent::text("(no output)"))
            } else {
                info!(terminal_id = %terminal_id, output_len = output.len(), "bash execute completed");
                Ok(ToolCallContent::text(output))
            }
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
    #[instrument(
        skip_all,
        fields(command, working_dir, timeout_ms, executor = "acp_bridge")
    )]
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
                "using fallback session_id='default'. \
                 acp_session_id not set in ToolCallContext — check RunOptions propagation chain"
            );
        }

        let (shell, args) = if cfg!(windows) {
            let wrapped = format!(
                "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {}",
                command
            );
            (
                "powershell".to_string(),
                vec!["-NoProfile".to_string(), "-Command".to_string(), wrapped],
            )
        } else {
            (
                "sh".to_string(),
                vec!["-c".to_string(), command.to_string()],
            )
        };

        info!(
            command = %command,
            session_id = %session_id,
            shell = %shell,
            working_dir = ?working_dir,
            timeout_ms = ?timeout_ms,
            env_count = env.len(),
            "bash execute called"
        );

        let bridge = crate::tools::get_session_bridge(session_id).await.map_err(|e| {
            error!(error = %e, "failed to get client bridge");
            ToolSourceError::Transport(e)
        })?;

        info!("client bridge acquired");

        let cwd = working_dir.map(|p| p.display().to_string());

        let terminal_id = bridge
            .terminal_create(session_id, &shell, args, env, cwd, None)
            .await
            .map_err(|e| {
                error!(session_id = %session_id, error = %e, "terminal create failed");
                ToolSourceError::Transport(e)
            })?;

        info!(terminal_id = %terminal_id, "terminal created via bridge");

        if let Some(timeout) = timeout_ms {
            info!(terminal_id = %terminal_id, timeout_ms = timeout, "waiting for exit with timeout");
            tokio::select! {
                result = bridge.terminal_wait_for_exit(session_id, &terminal_id) => {
                    match &result {
                        Ok(exit_result) => {
                            info!(terminal_id = %terminal_id, exit_code = ?exit_result.exit_code, signal = ?exit_result.signal, "process exited before timeout");
                        }
                        Err(e) => {
                            error!(terminal_id = %terminal_id, error = %e, "wait_for_exit failed");
                        }
                    }
                    let _ = result;
                }
                _ = tokio::time::sleep(Duration::from_millis(timeout)) => {
                    warn!(terminal_id = %terminal_id, timeout_ms = timeout, "command timed out, killing");
                    let _ = bridge.terminal_kill(session_id, &terminal_id).await;
                    let _ = bridge.terminal_release(session_id, &terminal_id).await;
                    return Err(ToolSourceError::Transport("Command timed out".into()));
                }
            }
        } else {
            info!(terminal_id = %terminal_id, "waiting for exit (no timeout)");
            let exit_result = bridge
                .terminal_wait_for_exit(session_id, &terminal_id)
                .await
                .map_err(|e| {
                    error!(terminal_id = %terminal_id, error = %e, "wait_for_exit failed");
                    ToolSourceError::Transport(format!("terminal wait: {}", e))
                })?;
            info!(terminal_id = %terminal_id, exit_code = ?exit_result.exit_code, signal = ?exit_result.signal, "wait_for_exit completed");
        }

        info!(terminal_id = %terminal_id, "fetching terminal output");
        let output = bridge
            .terminal_output(session_id, &terminal_id)
            .await
            .map_err(|e| {
                error!(terminal_id = %terminal_id, error = %e, "terminal_output failed");
                ToolSourceError::Transport(format!("terminal output: {}", e))
            })?;

        info!(
            terminal_id = %terminal_id,
            output_len = output.output.len(),
            truncated = output.truncated,
            "terminal output retrieved"
        );

        let _ = bridge.terminal_release(session_id, &terminal_id).await;
        info!(terminal_id = %terminal_id, "terminal released");

        if output.output.is_empty() {
            info!(terminal_id = %terminal_id, output_len = 0, "bash execute completed");
            Ok(ToolCallContent::text("(no output)"))
        } else {
            info!(terminal_id = %terminal_id, output_len = output.output.len(), "bash execute completed");
            Ok(ToolCallContent::text(output.output))
        }
    }
}
