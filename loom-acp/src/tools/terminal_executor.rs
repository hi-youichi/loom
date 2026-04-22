use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use loom::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use loom::tools::CommandExecutor;

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
            ("cmd".to_string(), vec!["/C".to_string(), command.to_string()])
        } else {
            ("sh".to_string(), vec!["-c".to_string(), command.to_string()])
        };

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
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;

        let result = if let Some(timeout) = timeout_ms {
            tokio::select! {
                status = self.terminal_mgr.wait_for_exit(&terminal_id) => {
                    status.map_err(|e| ToolSourceError::Transport(e.to_string()))
                }
                _ = tokio::time::sleep(Duration::from_millis(timeout)) => {
                    self.terminal_mgr.kill(&terminal_id).await.ok();
                    Err(ToolSourceError::Transport("Command timed out".into()))
                }
            }
        } else {
            self.terminal_mgr
                .wait_for_exit(&terminal_id)
                .await
                .map_err(|e| ToolSourceError::Transport(e.to_string()))
        };

        let _ = result;

        let (output, _truncated, _status) = self
            .terminal_mgr
            .get_output(&terminal_id)
            .await
            .unwrap_or_default();

        let _ = self.terminal_mgr.release(&terminal_id).await;

        if output.is_empty() {
            Ok(ToolCallContent::text("(no output)"))
        } else {
            Ok(ToolCallContent::text(output))
        }
    }
}
