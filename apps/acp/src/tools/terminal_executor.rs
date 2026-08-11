use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

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

pub struct AcpBridgeCommandExecutor {
    bridge: Arc<dyn crate::tools::ClientBridgeTrait>,
}

async fn cancellation_signal(token: Option<CancellationToken>) {
    if let Some(token) = token {
        token.cancelled().await;
    } else {
        std::future::pending::<()>().await;
    }
}

impl Default for AcpBridgeCommandExecutor {
    fn default() -> Self {
        Self::new(Arc::new(crate::tools::NoOpClientBridge))
    }
}

impl AcpBridgeCommandExecutor {
    pub fn new(bridge: Arc<dyn crate::tools::ClientBridgeTrait>) -> Self {
        Self { bridge }
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
            .ok_or_else(|| {
                ToolSourceError::Transport("acp_session_id not set in ToolCallContext".to_string())
            })?;

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

        let cwd = working_dir.map(|p| p.display().to_string());
        let cancellation = ctx
            .and_then(|context| context.run_cancellation.as_ref())
            .map(|cancellation| cancellation.token());

        let terminal_id = self
            .bridge
            .terminal_create(session_id, &shell, args, env, cwd, None)
            .await
            .map_err(|e| {
                error!(session_id = %session_id, error = %e, "terminal create failed");
                ToolSourceError::Transport(e)
            })?;

        info!(terminal_id = %terminal_id, "terminal created via bridge");

        if cancellation
            .as_ref()
            .map(CancellationToken::is_cancelled)
            .unwrap_or(false)
        {
            let _ = self.bridge.terminal_kill(session_id, &terminal_id).await;
            let _ = self.bridge.terminal_release(session_id, &terminal_id).await;
            return Err(ToolSourceError::Transport("Command cancelled".into()));
        }

        if let Some(timeout) = timeout_ms {
            info!(terminal_id = %terminal_id, timeout_ms = timeout, "waiting for exit with timeout");
            tokio::select! {
                result = self.bridge.terminal_wait_for_exit(session_id, &terminal_id) => {
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
                    let _ = self.bridge.terminal_kill(session_id, &terminal_id).await;
                    let _ = self.bridge.terminal_release(session_id, &terminal_id).await;
                    return Err(ToolSourceError::Transport("Command timed out".into()));
                }
                _ = cancellation_signal(cancellation.clone()) => {
                    warn!(terminal_id = %terminal_id, "command cancelled, killing terminal");
                    let _ = self.bridge.terminal_kill(session_id, &terminal_id).await;
                    let _ = self.bridge.terminal_release(session_id, &terminal_id).await;
                    return Err(ToolSourceError::Transport("Command cancelled".into()));
                }
            }
        } else {
            info!(terminal_id = %terminal_id, "waiting for exit (no timeout)");
            let exit_result = tokio::select! {
                result = self.bridge.terminal_wait_for_exit(session_id, &terminal_id) => result,
                _ = cancellation_signal(cancellation.clone()) => {
                    warn!(terminal_id = %terminal_id, "command cancelled, killing terminal");
                    let _ = self.bridge.terminal_kill(session_id, &terminal_id).await;
                    let _ = self.bridge.terminal_release(session_id, &terminal_id).await;
                    return Err(ToolSourceError::Transport("Command cancelled".into()));
                }
            }
            .map_err(|e| {
                error!(terminal_id = %terminal_id, error = %e, "wait_for_exit failed");
                ToolSourceError::Transport(format!("terminal wait: {}", e))
            });
            let exit_result = match exit_result {
                Ok(result) => result,
                Err(error) => {
                    let _ = self.bridge.terminal_kill(session_id, &terminal_id).await;
                    let _ = self.bridge.terminal_release(session_id, &terminal_id).await;
                    return Err(error);
                }
            };
            info!(terminal_id = %terminal_id, exit_code = ?exit_result.exit_code, signal = ?exit_result.signal, "wait_for_exit completed");
        }

        info!(terminal_id = %terminal_id, "fetching terminal output");
        let output = self
            .bridge
            .terminal_output(session_id, &terminal_id)
            .await
            .map_err(|e| {
                error!(terminal_id = %terminal_id, error = %e, "terminal_output failed");
                ToolSourceError::Transport(format!("terminal output: {}", e))
            });
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                let _ = self.bridge.terminal_kill(session_id, &terminal_id).await;
                let _ = self.bridge.terminal_release(session_id, &terminal_id).await;
                return Err(error);
            }
        };

        info!(
            terminal_id = %terminal_id,
            output_len = output.output.len(),
            truncated = output.truncated,
            "terminal output retrieved"
        );

        let _ = self.bridge.terminal_release(session_id, &terminal_id).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ClientBridgeTrait, TerminalExitResult, TerminalOutput};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::Notify;
    use tool_core::ToolCallContext;

    struct CancellationBridge {
        created: Arc<Notify>,
        killed: AtomicUsize,
        released: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ClientBridgeTrait for CancellationBridge {
        fn is_available(&self) -> bool {
            true
        }

        async fn read_text_file(
            &self,
            _path: &str,
            _line: Option<u32>,
            _limit: Option<u32>,
        ) -> Result<String, String> {
            Err("unused".into())
        }

        async fn write_text_file(&self, _path: &str, _content: &str) -> Result<(), String> {
            Err("unused".into())
        }

        async fn terminal_create(
            &self,
            _session_id: &str,
            _command: &str,
            _args: Vec<String>,
            _env: Vec<(String, String)>,
            _cwd: Option<String>,
            _output_byte_limit: Option<u64>,
        ) -> Result<String, String> {
            self.created.notify_one();
            Ok("terminal-cancel-test".into())
        }

        async fn terminal_output(
            &self,
            _session_id: &str,
            _terminal_id: &str,
        ) -> Result<TerminalOutput, String> {
            Err("unexpected output request".into())
        }

        async fn terminal_wait_for_exit(
            &self,
            _session_id: &str,
            _terminal_id: &str,
        ) -> Result<TerminalExitResult, String> {
            std::future::pending().await
        }

        async fn terminal_kill(&self, _session_id: &str, _terminal_id: &str) -> Result<(), String> {
            self.killed.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        async fn terminal_release(
            &self,
            _session_id: &str,
            _terminal_id: &str,
        ) -> Result<(), String> {
            self.released.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[tokio::test]
    async fn cancellation_kills_and_releases_waiting_terminal() {
        let bridge = Arc::new(CancellationBridge {
            created: Arc::new(Notify::new()),
            killed: AtomicUsize::new(0),
            released: AtomicUsize::new(0),
        });
        let cancellation = tool_core::active_operation::RunCancellation::new(1);
        let context = ToolCallContext {
            acp_session_id: Some("session-cancel-test".into()),
            run_cancellation: Some(cancellation.clone()),
            ..Default::default()
        };
        let executor = AcpBridgeCommandExecutor::new(bridge.clone());
        let task = tokio::spawn(async move {
            executor
                .execute("echo test", None, None, Vec::new(), Some(&context))
                .await
        });

        bridge.created.notified().await;
        cancellation.cancel();
        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("cancellation should finish the executor")
            .expect("executor task should not panic");

        assert!(result
            .expect_err("cancelled command must fail")
            .to_string()
            .contains("cancelled"));
        assert_eq!(bridge.killed.load(Ordering::Relaxed), 1);
        assert_eq!(bridge.released.load(Ordering::Relaxed), 1);
    }
}
