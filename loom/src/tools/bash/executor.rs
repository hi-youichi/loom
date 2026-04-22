use std::path::Path;

use async_trait::async_trait;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::{ActiveOperation, ActiveOperationCanceller, ActiveOperationKind};
use tokio::io::AsyncReadExt;
use tokio::sync::watch;

#[async_trait]
pub trait CommandExecutor: Send + Sync {
    async fn execute(
        &self,
        command: &str,
        working_dir: Option<&Path>,
        timeout_ms: Option<u64>,
        env: Vec<(String, String)>,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError>;
}

pub struct LocalCommandExecutor;

#[derive(Debug)]
struct ChildProcessCanceller {
    kill_tx: watch::Sender<bool>,
}

impl ActiveOperationCanceller for ChildProcessCanceller {
    fn cancel(&self) {
        let _ = self.kill_tx.send(true);
    }
}

#[async_trait]
impl CommandExecutor for LocalCommandExecutor {
    async fn execute(
        &self,
        command: &str,
        working_dir: Option<&Path>,
        timeout_ms: Option<u64>,
        _env: Vec<(String, String)>,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let workdir_str = working_dir.map(|p| p.to_string_lossy().into_owned());
        let timeout = timeout_ms.unwrap_or(0);
        let output = run_shell_command(command, workdir_str.as_deref(), timeout, ctx).await?;

        let text = if output.stderr.is_empty() {
            output.stdout
        } else if output.stdout.is_empty() {
            format!("stderr:\n{}", output.stderr)
        } else {
            format!("stdout:\n{}\nstderr:\n{}", output.stdout, output.stderr)
        };

        Ok(ToolCallContent::text(text))
    }
}

struct ShellOutput {
    stdout: String,
    stderr: String,
}

#[cfg(unix)]
async fn run_shell_command(
    command: &str,
    workdir: Option<&str>,
    timeout_ms: u64,
    ctx: Option<&ToolCallContext>,
) -> Result<ShellOutput, ToolSourceError> {
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(command);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    run_spawned_shell_command(cmd, timeout_ms, ctx).await
}

#[cfg(windows)]
async fn run_shell_command(
    command: &str,
    workdir: Option<&str>,
    timeout_ms: u64,
    ctx: Option<&ToolCallContext>,
) -> Result<ShellOutput, ToolSourceError> {
    let mut cmd = tokio::process::Command::new("cmd");
    cmd.args(["/C", command]);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    run_spawned_shell_command(cmd, timeout_ms, ctx).await
}

async fn run_spawned_shell_command(
    mut cmd: tokio::process::Command,
    timeout_ms: u64,
    ctx: Option<&ToolCallContext>,
) -> Result<ShellOutput, ToolSourceError> {
    let mut child = cmd
        .spawn()
        .map_err(|e| ToolSourceError::Transport(format!("failed to run command: {}", e)))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = tokio::spawn(async move { read_pipe(stdout).await });
    let stderr_reader = tokio::spawn(async move { read_pipe(stderr).await });

    let (kill_tx, mut kill_rx) = watch::channel(false);
    if let Some(run_cancellation) = ctx.and_then(|ctx| ctx.run_cancellation.clone()) {
        run_cancellation.set_active_operation(ActiveOperation::new(
            ActiveOperationKind::ChildProcess,
            std::sync::Arc::new(ChildProcessCanceller { kill_tx }),
        ));
    }

    let status = if timeout_ms == 0 {
        tokio::select! {
            _ = kill_rx.changed() => {
                let _ = child.kill().await;
                return Err(ToolSourceError::Transport("command cancelled".to_string()));
            }
            status = child.wait() => status,
        }
    } else {
        tokio::select! {
            _ = kill_rx.changed() => {
                let _ = child.kill().await;
                return Err(ToolSourceError::Transport("command cancelled".to_string()));
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)) => {
                let _ = child.kill().await;
                return Err(ToolSourceError::Transport("command timed out".to_string()));
            }
            status = child.wait() => status,
        }
    }
    .map_err(|e| ToolSourceError::Transport(format!("failed to run command: {}", e)))?;

    let stdout = stdout_reader
        .await
        .map_err(|e| ToolSourceError::Transport(format!("failed to read stdout: {}", e)))?;
    let stderr = stderr_reader
        .await
        .map_err(|e| ToolSourceError::Transport(format!("failed to read stderr: {}", e)))?;
    let _ = status;
    Ok(ShellOutput { stdout, stderr })
}

async fn read_pipe<R>(pipe: Option<R>) -> String
where
    R: tokio::io::AsyncRead + Unpin,
{
    if let Some(mut pipe) = pipe {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf).await;
        String::from_utf8_lossy(&buf).into_owned()
    } else {
        String::new()
    }
}
