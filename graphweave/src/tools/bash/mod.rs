//! Bash (shell) tool: run shell commands as an agent tool.
//!
//! Provides [`BashTool`] which executes a single shell command and returns
//! stdout and stderr. Uses `sh -c` on Unix and `cmd /C` on Windows.
//! Interacts with [`Tool`], [`ToolRegistry`](crate::tools::ToolRegistryLocked),
//! and [`AggregateToolSource`].

use async_trait::async_trait;

use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

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
/// use graphweave::tools::{BashTool, Tool};
/// use serde_json::json;
///
/// # #[tokio::main]
/// # async fn main() {
/// let tool = BashTool::new();
/// let args = json!({ "command": "echo hello" });
/// let result = tool.call(args, None).await.unwrap();
/// assert!(result.text.contains("hello"));
/// # }
/// ```
///
/// # Interaction
///
/// - **Tool**: Implements this trait for registration with [`AggregateToolSource`].
/// - **ToolSourceError**: Invalid input or command execution failure.
/// - **ToolCallContext**: Not used by this tool.
pub struct BashTool;

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

impl BashTool {
    /// Creates a new BashTool.
    ///
    /// # Examples
    ///
    /// ```
    /// use graphweave::tools::bash::BashTool;
    ///
    /// let tool = BashTool::new();
    /// ```
    pub fn new() -> Self {
        Self
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
        }
    }

    /// Executes the tool by running the given command in the system shell.
    ///
    /// # Parameters
    ///
    /// - `args`: JSON with required `"command"` string.
    /// - `_ctx`: Optional per-call context (not used by this tool).
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
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing command".to_string()))?;
        let workdir = args.get("workdir").and_then(|v| v.as_str());
        let timeout_ms = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(120_000);

        let output = run_shell_command(command, workdir, timeout_ms).await?;

        let text = if output.stderr.is_empty() {
            output.stdout
        } else if output.stdout.is_empty() {
            format!("stderr:\n{}", output.stderr)
        } else {
            format!("stdout:\n{}\nstderr:\n{}", output.stdout, output.stderr)
        };

        Ok(ToolCallContent { text })
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
) -> Result<ShellOutput, ToolSourceError> {
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(command);
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    let output = if timeout_ms == 0 {
        cmd.output()
            .await
            .map_err(|e| ToolSourceError::Transport(format!("failed to run command: {}", e)))?
    } else {
        tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            cmd.output(),
        )
        .await
        .map_err(|_| ToolSourceError::Transport("command timed out".to_string()))?
        .map_err(|e| ToolSourceError::Transport(format!("failed to run command: {}", e)))?
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok(ShellOutput { stdout, stderr })
}

#[cfg(windows)]
async fn run_shell_command(
    command: &str,
    workdir: Option<&str>,
    timeout_ms: u64,
) -> Result<ShellOutput, ToolSourceError> {
    let mut cmd = tokio::process::Command::new("cmd");
    cmd.args(["/C", command]);
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    let output = if timeout_ms == 0 {
        cmd.output()
            .await
            .map_err(|e| ToolSourceError::Transport(format!("failed to run command: {}", e)))?
    } else {
        tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            cmd.output(),
        )
        .await
        .map_err(|_| ToolSourceError::Transport("command timed out".to_string()))?
        .map_err(|e| ToolSourceError::Transport(format!("failed to run command: {}", e)))?
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok(ShellOutput { stdout, stderr })
}
