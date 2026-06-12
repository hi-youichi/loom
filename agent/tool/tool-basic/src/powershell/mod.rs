//! PowerShell tool: run PowerShell commands on Windows.
//!
//! Uses `pwsh -Command` when available, otherwise `powershell -Command` (5.1).
//! Shell choice is cached after the first probe. Cancellation and timeouts follow
//! the same pattern as [`tool_core::BashTool`].

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

use serde_json::json;

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use tool_core::Tool;
use tool_core::{ToolOutputHint, ToolOutputStrategy};

mod executor;
pub use executor::LocalPowerShellExecutor;

#[async_trait]
#[allow(clippy::too_many_arguments)]
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

pub const TOOL_POWERSHELL: &str = "powershell";

pub struct PowerShellTool {
    working_folder: Option<Arc<std::path::PathBuf>>,
    executor: Arc<dyn PowerShellExecutor>,
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
            executor: Arc::new(LocalPowerShellExecutor),
        }
    }

    pub fn with_working_folder(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self {
            working_folder: Some(working_folder),
            executor: Arc::new(LocalPowerShellExecutor),
        }
    }

    pub fn with_executor(executor: Arc<dyn PowerShellExecutor>) -> Self {
        Self {
            working_folder: None,
            executor,
        }
    }

    pub fn with_working_folder_and_executor(
        working_folder: Arc<std::path::PathBuf>,
        executor: Arc<dyn PowerShellExecutor>,
    ) -> Self {
        Self {
            working_folder: Some(working_folder),
            executor,
        }
    }
}

#[async_trait]
impl Tool for PowerShellTool {
    fn name(&self) -> &str {
        TOOL_POWERSHELL
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_POWERSHELL.to_string(),
            description: Some(
                "Executes a PowerShell command on Windows (WMI, Registry, .NET, COM). \
                 Uses pwsh when installed, else Windows PowerShell 5.1. \
                 This is the primary shell tool on Windows (bash is not available on Windows). \
                 Timeout is in milliseconds (default 120000)."
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
            output_hint: Some(
                ToolOutputHint::preferred(ToolOutputStrategy::HeadTail).prefer_head_tail(),
            ),
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
        let working_dir = match workdir_arg {
            Some(dir) => Some(std::path::PathBuf::from(dir)),
            None => self
                .working_folder
                .as_ref()
                .map(|p| p.as_ref().clone()),
        };

        let timeout_ms = args
            .get("timeout")
            .or_else(|| args.get("timeout_ms"))
            .and_then(|v| v.as_u64());

        let use_legacy = args
            .get("use_legacy_powershell")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let execution_policy = args
            .get("execution_policy")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        let env_pairs = parse_env_object(args.get("env"))?;

        self.executor
            .execute(
                command,
                working_dir.as_deref(),
                timeout_ms,
                env_pairs,
                execution_policy,
                use_legacy,
                ctx,
            )
            .await
    }
}

fn parse_env_object(
    v: Option<&serde_json::Value>,
) -> Result<Vec<(String, String)>, ToolSourceError> {
    let Some(v) = v else {
        return Ok(Vec::new());
    };
    let obj = v.as_object().ok_or_else(|| {
        ToolSourceError::InvalidInput(
            "'env' must be a JSON object of string keys to string values".to_string(),
        )
    })?;
    let mut out = Vec::with_capacity(obj.len());
    for (k, val) in obj {
        let s = val.as_str().ok_or_else(|| {
            ToolSourceError::InvalidInput(format!("env value for {:?} must be a string", k))
        })?;
        out.push((k.clone(), s.to_string()));
    }
    Ok(out)
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
}
