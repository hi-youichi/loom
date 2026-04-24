use std::path::Path;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::{ActiveOperation, ActiveOperationCanceller, ActiveOperationKind};
use tokio::io::AsyncReadExt;
use tokio::sync::watch;

#[async_trait]
pub trait PowerShellExecutor: Send + Sync {
    async fn execute(
        &self,
        command: &str,
        working_dir: Option<&Path>,
        timeout_ms: Option<u64>,
        env: Vec<(String, String)>,
        execution_policy: Option<&str>,
        use_legacy: bool,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError>;
}

pub struct LocalPowerShellExecutor;

#[derive(Debug)]
struct ChildProcessCanceller {
    kill_tx: watch::Sender<bool>,
}

impl ActiveOperationCanceller for ChildProcessCanceller {
    fn cancel(&self) {
        let _ = self.kill_tx.send(true);
    }
}

fn detect_powershell() -> (&'static str, &'static str) {
    static CACHED: OnceLock<(&'static str, &'static str)> = OnceLock::new();
    *CACHED.get_or_init(|| {
        let pwsh_ok = std::process::Command::new("pwsh")
            .args(["-NoProfile", "-NonInteractive", "-Command", "exit 0"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if pwsh_ok {
            ("pwsh", "-Command")
        } else {
            ("powershell", "-Command")
        }
    })
}

#[async_trait]
impl PowerShellExecutor for LocalPowerShellExecutor {
    async fn execute(
        &self,
        command: &str,
        working_dir: Option<&Path>,
        timeout_ms: Option<u64>,
        env: Vec<(String, String)>,
        execution_policy: Option<&str>,
        use_legacy: bool,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let workdir_str = working_dir.map(|p| p.to_string_lossy().into_owned());
        let timeout = timeout_ms.unwrap_or(120_000);

        let (shell, shell_cmd_arg) = if use_legacy {
            ("powershell", "-Command")
        } else {
            detect_powershell()
        };

        let text = run_powershell_command(
            shell,
            shell_cmd_arg,
            command,
            workdir_str.as_deref(),
            &env,
            execution_policy,
            timeout,
            ctx,
        )
        .await?;

        Ok(ToolCallContent::text(text))
    }
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

#[allow(clippy::too_many_arguments)]
async fn run_powershell_command(
    shell: &str,
    shell_cmd_arg: &str,
    command: &str,
    workdir: Option<&str>,
    env_pairs: &[(String, String)],
    execution_policy: Option<&str>,
    timeout_ms: u64,
    ctx: Option<&ToolCallContext>,
) -> Result<String, ToolSourceError> {
    let mut cmd = tokio::process::Command::new(shell);
    if let Some(ep) = execution_policy {
        cmd.arg("-ExecutionPolicy").arg(ep);
    }
    cmd.arg(shell_cmd_arg).arg(command);
    for (k, v) in env_pairs {
        cmd.env(k, v);
    }
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| ToolSourceError::Transport(format!("failed to spawn PowerShell: {}", e)))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = tokio::spawn(async move { read_pipe(stdout).await });
    let stderr_reader = tokio::spawn(async move { read_pipe(stderr).await });

    let (kill_tx, mut kill_rx) = watch::channel(false);
    if let Some(run_cancellation) = ctx.and_then(|c| c.run_cancellation.clone()) {
        run_cancellation.set_active_operation(ActiveOperation::new(
            ActiveOperationKind::ChildProcess,
            Arc::new(ChildProcessCanceller { kill_tx }),
        ));
    }

    let status = if timeout_ms == 0 {
        tokio::select! {
            _ = kill_rx.changed() => {
                let _ = child.kill().await;
                return Err(ToolSourceError::Transport("PowerShell command cancelled".to_string()));
            }
            status = child.wait() => status,
        }
    } else {
        tokio::select! {
            _ = kill_rx.changed() => {
                let _ = child.kill().await;
                return Err(ToolSourceError::Transport("PowerShell command cancelled".to_string()));
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)) => {
                let _ = child.kill().await;
                return Err(ToolSourceError::Transport(format!(
                    "PowerShell command timed out after {} ms",
                    timeout_ms
                )));
            }
            status = child.wait() => status,
        }
    }
    .map_err(|e| ToolSourceError::Transport(format!("failed to wait for PowerShell: {}", e)))?;

    let stdout = stdout_reader
        .await
        .map_err(|e| ToolSourceError::Transport(format!("failed to read stdout: {}", e)))?;
    let stderr = stderr_reader
        .await
        .map_err(|e| ToolSourceError::Transport(format!("failed to read stderr: {}", e)))?;

    let mut text = if stderr.is_empty() {
        stdout
    } else if stdout.is_empty() {
        format!("stderr:\n{}", stderr)
    } else {
        format!("stdout:\n{}\nstderr:\n{}", stdout, stderr)
    };

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        text.push_str(&format!("\n[PowerShell exited with code {}]", code));
    }

    Ok(text)
}
