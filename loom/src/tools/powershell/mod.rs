//! PowerShell tool: run PowerShell commands on Windows.
//!
//! Uses `pwsh -Command` when available, otherwise `powershell -Command` (5.1).
//! Shell choice is cached after the first probe. Cancellation and timeouts follow
//! the same pattern as [`crate::tools::BashTool`].

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;

use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::sync::watch;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;
use crate::{ActiveOperation, ActiveOperationCanceller, ActiveOperationKind};
use crate::{ToolOutputHint, ToolOutputStrategy};

/// Tool name for the PowerShell execution operation.
pub const TOOL_POWERSHELL: &str = "powershell";

/// Tool that runs PowerShell commands and returns stdout and stderr.
///
/// Registered only on Windows in the ReAct tool source; on other platforms use `bash`.
pub struct PowerShellTool {
    working_folder: Option<Arc<std::path::PathBuf>>,
}

#[derive(Debug)]
struct ChildProcessCanceller {
    kill_tx: watch::Sender<bool>,
}

impl ActiveOperationCanceller for ChildProcessCanceller {
    fn cancel(&self) {
        let _ = self.kill_tx.send(true);
    }
}

impl Default for PowerShellTool {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerShellTool {
    pub fn new() -> Self {
        Self {
            working_folder: None,
        }
    }

    pub fn with_working_folder(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self {
            working_folder: Some(working_folder),
        }
    }

    /// Prefer `pwsh` when it runs; otherwise `powershell` (Windows PowerShell 5.1).
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
}

#[async_trait]
impl Tool for PowerShellTool {
    fn name(&self) -> &str {
        TOOL_POWERSHELL
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_POWERSHELL.to_string(),
            description: Some(
                "Executes a PowerShell command on Windows (WMI, Registry, .NET, COM). \
                 Uses pwsh when installed, else Windows PowerShell 5.1. \
                 For git/npm/cargo and cross-platform shell, prefer the bash tool. \
                 Timeout is in milliseconds (default 120000), same as bash."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "PowerShell script or expression to run."
                    },
                    "workdir": {
                        "type": "string",
                        "description": "Working directory (optional; defaults to agent working folder if set)."
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in milliseconds (default 120000). Use 0 for no limit."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Alias for timeout (milliseconds)."
                    },
                    "env": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "description": "Extra environment variables for the process."
                    },
                    "execution_policy": {
                        "type": "string",
                        "description": "Optional -ExecutionPolicy value (e.g. Bypass, RemoteSigned)."
                    },
                    "use_legacy_powershell": {
                        "type": "boolean",
                        "description": "If true, force Windows PowerShell 5.1 (powershell.exe) instead of pwsh."
                    }
                },
                "required": ["command"]
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::HeadTail).prefer_head_tail()),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing command".to_string()))?;

        let workdir_arg = args.get("workdir").and_then(|v| v.as_str());
        let workdir = match workdir_arg {
            Some(w) => Some(w.to_string()),
            None => self
                .working_folder
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
        };

        let timeout_ms = args
            .get("timeout")
            .or_else(|| args.get("timeout_ms"))
            .and_then(|v| v.as_u64())
            .unwrap_or(120_000);

        let use_legacy = args
            .get("use_legacy_powershell")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let execution_policy = args
            .get("execution_policy")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        let env_pairs = parse_env_object(args.get("env"))?;

        let (shell, shell_cmd_arg) = if use_legacy {
            ("powershell", "-Command")
        } else {
            Self::detect_powershell()
        };

        let text = run_powershell_command(
            shell,
            shell_cmd_arg,
            command,
            workdir.as_deref(),
            &env_pairs,
            execution_policy,
            timeout_ms,
            ctx,
        )
        .await?;

        Ok(ToolCallContent { text })
    }
}

fn parse_env_object(v: Option<&serde_json::Value>) -> Result<Vec<(String, String)>, ToolSourceError> {
    let Some(v) = v else {
        return Ok(Vec::new());
    };
    let obj = v.as_object().ok_or_else(|| {
        ToolSourceError::InvalidInput("'env' must be a JSON object of string keys to string values".to_string())
    })?;
    let mut out = Vec::with_capacity(obj.len());
    for (k, val) in obj {
        let s = val.as_str().ok_or_else(|| {
            ToolSourceError::InvalidInput(format!(
                "env value for {:?} must be a string",
                k
            ))
        })?;
        out.push((k.clone(), s.to_string()));
    }
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_name() {
        let tool = PowerShellTool::new();
        assert_eq!(tool.name(), "powershell");
    }

    #[test]
    fn test_spec_has_required_fields() {
        let tool = PowerShellTool::new();
        let spec = tool.spec();
        assert_eq!(spec.name, "powershell");
        assert!(spec.description.is_some());
        assert!(spec.output_hint.is_some());
    }

    #[test]
    fn test_detect_powershell_returns_valid() {
        let (shell, arg) = PowerShellTool::detect_powershell();
        assert!(!shell.is_empty());
        assert_eq!(arg, "-Command");
        assert!(shell == "pwsh" || shell == "powershell");
    }
}
