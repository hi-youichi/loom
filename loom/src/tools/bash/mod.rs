//! Bash (shell) tool: run shell commands as an agent tool.
//!
//! Provides [`BashTool`] which executes a single shell command and returns
//! stdout and stderr. Uses `sh -c` on Unix and `cmd /C` on Windows.
//! Interacts with [`Tool`], [`ToolRegistry`](crate::tools::ToolRegistryLocked),
//! and [`AggregateToolSource`].

use std::sync::Arc;

use async_trait::async_trait;

use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::sync::watch;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;
use crate::{ActiveOperation, ActiveOperationCanceller, ActiveOperationKind};
use crate::{ToolOutputHint, ToolOutputStrategy};

/// Tool name for the bash/shell execution operation.
pub const TOOL_BASH: &str = "bash";

/// Tool that runs a shell command and returns stdout and stderr.
///
/// Executes the given command string via the system shell (`sh -c` on Unix,
/// `cmd /C` on Windows). Intended for use by agents that need to run system
/// commands. Use with care: this runs in the process environment and can
/// execute arbitrary code.
///
/// # Examples
///
/// ```no_run
/// use loom::tools::{BashTool, Tool};
/// use serde_json::json;
/// use std::sync::Arc;
/// use std::path::PathBuf;
///
/// # #[tokio::main]
/// # async fn main() {
/// let tool = BashTool::new();
/// let args = json!({ "command": "echo hello" });
/// let result = tool.call(args, None).await.unwrap();
/// assert!(result.as_text().unwrap().contains("hello"));
///
/// // With working folder
/// let tool = BashTool::with_working_folder(Arc::new(PathBuf::from("/tmp")));
/// # }
/// ```
///
/// # Interaction
///
/// - **Tool**: Implements this trait for registration with [`AggregateToolSource`].
/// - **ToolSourceError**: Invalid input or command execution failure.
/// - **ToolCallContext**: When `run_cancellation` is set (ReAct act node), registers
///   child-process cancellation so user cancel kills the shell (`sh`/`cmd`) subprocess.
pub struct BashTool {
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

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

impl BashTool {
    /// Creates a new BashTool without a default working folder.
    ///
    /// Commands will run in the process's current directory unless `workdir` is specified.
    pub fn new() -> Self {
        Self { working_folder: None }
    }

    /// Creates a new BashTool with a default working folder.
    ///
    /// When `workdir` is not specified in the call, commands will run in this folder.
    pub fn with_working_folder(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder: Some(working_folder) }
    }
}

#[async_trait]
impl Tool for BashTool {
    /// Returns the unique name of this tool.
    ///
    /// Returns `"bash"` as the tool identifier.
    fn name(&self) -> &str {
        TOOL_BASH
    }

    /// Returns the specification for this tool.
    ///
    /// Includes tool name, description for the LLM, and JSON schema with
    /// required `command` parameter.
    ///
    /// # Interaction
    ///
    /// - Called by [`ToolRegistry::list`](crate::tools::ToolRegistryLocked) to build `Vec<ToolSpec>`.
    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_BASH.to_string(),
            description: Some(
                "Executes a shell command in a subprocess with optional workdir and timeout. \
                 Use for git, npm, cargo, docker, etc. Do NOT use for file read/write/search — use read, grep, glob, edit instead. \
                 On Unix uses sh -c; on Windows uses cmd /C. Returns combined stdout and stderr."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to run."
                    },
                    "workdir": {
                        "type": "string",
                        "description": "Directory to run in (relative or absolute). Omit for working folder."
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in milliseconds (default 120000).",
                        "default": 120000
                    },
                    "description": {
                        "type": "string",
                        "description": "Short description of the command (5-10 words)."
                    }
                },
                "required": ["command"]
            }),
            output_hint: Some(
                ToolOutputHint::preferred(ToolOutputStrategy::HeadTail).prefer_head_tail(),
            ),
        }
    }

    /// Executes the tool by running the given command in the system shell.
    ///
    /// # Parameters
    ///
    /// - `args`: JSON with required `"command"` string.
    /// - `ctx`: Optional per-call context; `run_cancellation` enables killing the shell on cancel.
    ///
    /// # Returns
    ///
    /// Combined stdout and stderr as text. If both are non-empty, format is
    /// "stdout:\n{stdout}\nstderr:\n{stderr}".
    ///
    /// # Errors
    ///
    /// - [`ToolSourceError::InvalidInput`] if `command` is missing or not a string.
    /// - [`ToolSourceError::Transport`] if the process fails to start or times out.
    ///
    /// # Interaction
    ///
    /// - Called by [`ToolRegistry::call`] when the tool is invoked.
    /// - Uses `tokio::process::Command` for async execution.
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
        let timeout_ms = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(120_000);

        let workdir = match workdir_arg {
            Some(w) => Some(w.to_string()),
            None => self.working_folder.as_ref().map(|p| p.to_string_lossy().into_owned()),
        };
        let workdir_str = workdir.as_deref();

        let output = run_shell_command(command, workdir_str, timeout_ms, ctx).await?;

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

/// Result of running a shell command (stdout and stderr).
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
            Arc::new(ChildProcessCanceller { kill_tx }),
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
