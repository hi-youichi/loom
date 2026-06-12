use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tracing::{debug, error, info, instrument, warn};

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::shared::canceller::setup_cancellation;
use crate::shared::shell_output::{
    ShellOutput, create_output_file, format_shell_output, generate_run_id, make_relative,
    shell_output_dir,
};
use tokio::sync::watch;

const DEFAULT_TIMEOUT_MS: u64 = 120_000;

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

#[async_trait]
impl CommandExecutor for LocalCommandExecutor {
    #[instrument(skip_all, fields(command, working_dir, timeout_ms))]
    async fn execute(
        &self,
        command: &str,
        working_dir: Option<&Path>,
        timeout_ms: Option<u64>,
        _env: Vec<(String, String)>,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let timeout = match timeout_ms {
            Some(0) | None => DEFAULT_TIMEOUT_MS,
            Some(ms) => ms,
        };

        info!(
            command = %command,
            working_dir = ?working_dir,
            timeout_ms = timeout,
            env_count = _env.len(),
            "bash execute called (local executor)"
        );

        let output = run_shell_command(command, working_dir, timeout, ctx).await?;
        let text = format_shell_output(&output);

        info!(
            stdout_len = output.stdout.len(),
            stderr_len = output.stderr.len(),
            timed_out = output.timed_out,
            output_len = text.len(),
            "bash execute completed"
        );

        Ok(ToolCallContent::text(text))
    }
}

#[cfg(unix)]
#[instrument(skip_all, fields(command, workdir, timeout_ms))]
async fn run_shell_command(
    command: &str,
    workdir: Option<&Path>,
    timeout_ms: u64,
    ctx: Option<&ToolCallContext>,
) -> Result<ShellOutput, ToolSourceError> {
    debug!(
        shell = "sh",
        command = %command,
        workdir = ?workdir,
        "spawning shell command"
    );

    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(command);
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    run_spawned_shell_command(cmd, workdir, timeout_ms, ctx).await
}

#[cfg(windows)]
#[instrument(skip_all, fields(command, workdir, timeout_ms))]
async fn run_shell_command(
    command: &str,
    workdir: Option<&Path>,
    timeout_ms: u64,
    ctx: Option<&ToolCallContext>,
) -> Result<ShellOutput, ToolSourceError> {
    debug!(
        shell = "powershell",
        command = %command,
        workdir = ?workdir,
        "spawning shell command"
    );

    let mut cmd = tokio::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-Command", command]);
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    run_spawned_shell_command(cmd, workdir, timeout_ms, ctx).await
}

async fn run_spawned_shell_command(
    mut cmd: tokio::process::Command,
    workdir: Option<&Path>,
    timeout_ms: u64,
    ctx: Option<&ToolCallContext>,
) -> Result<ShellOutput, ToolSourceError> {
    debug!("preparing shell command with file redirect");

    let base_dir = workdir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let shell_dir = shell_output_dir(&base_dir);
    tokio::fs::create_dir_all(&shell_dir)
        .await
        .map_err(|e| {
            error!(error = %e, path = %shell_dir.display(), "failed to create shell output directory");
            ToolSourceError::Transport(format!("failed to create output directory: {}", e))
        })?;

    let run_id = generate_run_id();
    let stdout_path = shell_dir.join(format!("{}.stdout", run_id));
    let stderr_path = shell_dir.join(format!("{}.stderr", run_id));

    let stdout_file = create_output_file(&stdout_path).map_err(|e| {
        error!(error = %e, path = %stdout_path.display(), "failed to create stdout file");
        ToolSourceError::Transport(format!("failed to create output file: {}", e))
    })?;
    let stderr_file = create_output_file(&stderr_path).map_err(|e| {
        error!(error = %e, path = %stderr_path.display(), "failed to create stderr file");
        let _ = std::fs::remove_file(&stdout_path);
        ToolSourceError::Transport(format!("failed to create output file: {}", e))
    })?;

    cmd.stdout(std::process::Stdio::from(stdout_file));
    cmd.stderr(std::process::Stdio::from(stderr_file));
    cmd.stdin(std::process::Stdio::piped());

    debug!("spawning child process");
    let mut child = cmd.spawn().map_err(|e| {
        error!(error = %e, "failed to spawn child process");
        let _ = std::fs::remove_file(&stdout_path);
        let _ = std::fs::remove_file(&stderr_path);
        ToolSourceError::Transport(format!("failed to run command: {}", e))
    })?;

    let pid = child.id();
    debug!(pid = ?pid, "child process spawned");

    let (kill_tx, mut kill_rx) = watch::channel(false);
    let _kill_tx_guard = setup_cancellation(ctx, kill_tx);
    let _ = kill_rx.borrow_and_update();

    debug!(
        pid = ?pid,
        timeout_ms = timeout_ms,
        "waiting for child process"
    );

    let result = tokio::select! {
        _ = kill_rx.changed() => {
            warn!(pid = ?pid, "command cancelled");
            let _ = child.kill().await;
            let _ = tokio::fs::remove_file(&stdout_path).await;
            let _ = tokio::fs::remove_file(&stderr_path).await;
            return Err(ToolSourceError::Transport("command cancelled".to_string()));
        }
        _ = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)) => {
            warn!(pid = ?pid, timeout_ms = timeout_ms, "command timed out, detaching process");
            let pid_val = child.id().unwrap_or(pid.unwrap_or(0));
            let _ = child.stdin.take();
            std::mem::forget(child);

            let partial_stdout = tokio::fs::read_to_string(&stdout_path)
                .await
                .unwrap_or_default();
            let partial_stderr = tokio::fs::read_to_string(&stderr_path)
                .await
                .unwrap_or_default();

            let stdout_rel = make_relative(&stdout_path, &base_dir);
            let stderr_rel = make_relative(&stderr_path, &base_dir);

            info!(
                pid = pid_val,
                stdout_file = %stdout_rel.display(),
                "process detached, output written to files"
            );

            return Ok(ShellOutput {
                stdout: partial_stdout,
                stderr: partial_stderr,
                pid: Some(pid_val),
                timed_out: true,
                stdout_file: Some(stdout_rel),
                stderr_file: Some(stderr_rel),
            });
        }
        status = child.wait() => status,
    };

    let status = result.map_err(|e| {
        error!(pid = ?pid, error = %e, "failed to wait for child process");
        ToolSourceError::Transport(format!("failed to run command: {}", e))
    })?;

    debug!(
        pid = ?pid,
        exit_code = status.code(),
        "child process exited"
    );

    let stdout = tokio::fs::read_to_string(&stdout_path).await.unwrap_or_default();
    let stderr = tokio::fs::read_to_string(&stderr_path).await.unwrap_or_default();

    let _ = tokio::fs::remove_file(&stdout_path).await;
    let _ = tokio::fs::remove_file(&stderr_path).await;

    debug!(
        stdout_len = stdout.len(),
        stderr_len = stderr.len(),
        "output collected from files"
    );

    Ok(ShellOutput {
        stdout,
        stderr,
        pid: None,
        timed_out: false,
        stdout_file: None,
        stderr_file: None,
    })
}