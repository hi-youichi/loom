use std::sync::Arc;

use async_trait::async_trait;

use serde_json::json;

use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

mod executor;
pub use executor::{CommandExecutor, LocalCommandExecutor};

pub use tool_core::tool_name::TOOL_BASH;

pub struct BashTool {
    working_folder: Option<Arc<std::path::PathBuf>>,
    executor: Arc<dyn CommandExecutor>,
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            working_folder: None,
            executor: Arc::new(LocalCommandExecutor),
        }
    }

    pub fn with_working_folder(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self {
            working_folder: Some(working_folder),
            executor: Arc::new(LocalCommandExecutor),
        }
    }

    pub fn with_executor(executor: Arc<dyn CommandExecutor>) -> Self {
        Self {
            working_folder: None,
            executor,
        }
    }

    pub fn with_working_folder_and_executor(
        working_folder: Arc<std::path::PathBuf>,
        executor: Arc<dyn CommandExecutor>,
    ) -> Self {
        Self {
            working_folder: Some(working_folder),
            executor,
        }
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        TOOL_BASH
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_BASH.to_string(),
            description: Some(
                "Executes a bash/shell command on the local machine and returns the output. \
                 Use this tool to run shell commands, scripts, install packages, etc. \
                 When a command exceeds the timeout, it continues running in the background. \
                 The tool returns the PID and output file paths. \
                 Use `cat <stdout_file>` to read the latest output, `kill <PID>` to stop the process, \
                 `kill -0 <PID>` to check if the process is still running."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "workdir": {
                        "type": "string",
                        "description": "Working directory for the command. If not specified, uses the working folder."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Timeout in milliseconds. Default 120000 (120 seconds). The command runs in the background after timeout."
                    },
                    "description": {
                        "type": "string",
                        "description": "A short description of the command for context"
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
            .ok_or_else(|| ToolSourceError::InvalidInput("missing 'command' field".to_string()))?;

        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .or(Some(120_000));

        let workdir = args.get("workdir").and_then(|v| v.as_str());
        let working_dir = match workdir {
            Some(dir) => Some(std::path::PathBuf::from(dir)),
            None => self.working_folder.as_ref().map(|p| p.as_ref().clone()),
        };

        self.executor
            .execute(command, working_dir.as_deref(), timeout_ms, vec![], ctx)
            .await
    }
}
