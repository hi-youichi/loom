use std::path::Path;
use std::sync::OnceLock;

use async_trait::async_trait;
use tracing::{debug, error, info, warn};

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::shared::canceller::setup_cancellation;
use crate::tools::shared::shell_output::{
    ShellOutput, create_output_file, format_shell_output, generate_run_id, make_relative,
    shell_output_dir,
};
use tokio::sync::watch;

use super::PowerShellExecutor;


pub struct LocalPowerShellExecutor;

static CACHED_SHELL: OnceLock<(&'static str, &'static str)> = OnceLock::new();

fn detect_powershell() -> (&'static str, &'static str) {
    *CACHED_SHELL.get_or_init(|| {
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
        let timeout = timeout_ms.unwrap_or(120_000);

        let (shell, shell_cmd_arg) = if use_legacy {
            ("powershell", "-Command")
        } else {
            detect_powershell()
        };

        let output = run_powershell_command(
            shell,
            shell_cmd_arg,
            command,
            working_dir,
            &env,
            execution_policy,
            timeout,
            ctx,
        )
        .await?;

        let text = format_shell_output(&output);

        info!(
            stdout_len = output.stdout.len(),
            stderr_len = output.stderr.len(),
            timed_out = output.timed_out,
            output_len = text.len(),
            "powershell execute completed"
        );

        Ok(ToolCallContent::text(text))
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_powershell_command(
    shell: &str,
    shell_cmd_arg: &str,
    command: &str,
    working_dir: Option<&Path>,
    env_pairs: &[(String, String)],
    execution_policy: Option<&str>,
    timeout_ms: u64,
    ctx: Option<&ToolCallContext>,
) -> Result<ShellOutput, ToolSourceError> {
    let base_dir = working_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")));

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

    let mut cmd = tokio::process::Command::new(shell);
    if let Some(ep) = execution_policy {
        cmd.arg("-ExecutionPolicy").arg(ep);
    }
    cmd.arg(shell_cmd_arg).arg(command);
    for (k, v) in env_pairs {
        cmd.env(k, v);
    }
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::from(stdout_file));
    cmd.stderr(std::process::Stdio::from(stderr_file));

    debug!(shell = %shell, command = %command, "spawning powershell command with file redirect");

    let mut child = cmd.spawn().map_err(|e| {
        error!(error = %e, "failed to spawn PowerShell");
        let _ = std::fs::remove_file(&stdout_path);
        let _ = std::fs::remove_file(&stderr_path);
        ToolSourceError::Transport(format!("failed to spawn PowerShell: {}", e))
    })?;

    let pid = child.id();
    debug!(pid = ?pid, shell = %shell, "powershell process spawned");

    let (kill_tx, mut kill_rx) = watch::channel(false);
    let _kill_tx_guard = setup_cancellation(ctx, kill_tx);
    let _ = kill_rx.borrow_and_update();

    debug!(pid = ?pid, timeout_ms = timeout_ms, "waiting for powershell process");

    let result = tokio::select! {
        _ = kill_rx.changed() => {
            warn!(pid = ?pid, "powershell command cancelled");
            let _ = child.kill().await;
            let _ = tokio::fs::remove_file(&stdout_path).await;
            let _ = tokio::fs::remove_file(&stderr_path).await;
            return Err(ToolSourceError::Transport("PowerShell command cancelled".to_string()));
        }
        _ = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)) => {
            warn!(pid = ?pid, timeout_ms = timeout_ms, "powershell command timed out, detaching process");
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
                "powershell process detached, output written to files"
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
        error!(pid = ?pid, error = %e, "failed to wait for powershell process");
        ToolSourceError::Transport(format!("failed to wait for PowerShell: {}", e))
    })?;

    debug!(
        pid = ?pid,
        exit_code = status.code(),
        "powershell process exited"
    );

    let stdout = tokio::fs::read_to_string(&stdout_path).await.unwrap_or_default();
    let stderr = tokio::fs::read_to_string(&stderr_path).await.unwrap_or_default();

    let _ = tokio::fs::remove_file(&stdout_path).await;
    let _ = tokio::fs::remove_file(&stderr_path).await;

    let mut output = ShellOutput {
        stdout,
        stderr,
        pid: None,
        timed_out: false,
        stdout_file: None,
        stderr_file: None,
    };

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        output.stderr.push_str(&format!(
            "\n[{} exited with code {}]",
            shell, code
        ));
    }

    Ok(output)
}