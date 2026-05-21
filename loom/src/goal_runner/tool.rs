use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::cli_run::AnyStreamEvent;

use super::state::{ToolError, TurnResult};

const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 300;

#[async_trait]
pub trait CodingTool: Send + Sync {
    async fn execute(&self, prompt: &str, working_dir: &Path) -> Result<TurnResult, ToolError>;
    fn name(&self) -> &str;
}

pub struct ShellTool {
    command: String,
    args: Vec<String>,
    timeout: Duration,
}

impl ShellTool {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self {
            command,
            args,
            timeout: Duration::from_secs(DEFAULT_TOOL_TIMEOUT_SECS),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl CodingTool for ShellTool {
    async fn execute(&self, prompt: &str, working_dir: &Path) -> Result<TurnResult, ToolError> {
        let result = tokio::time::timeout(self.timeout, async {
            let mut cmd = tokio::process::Command::new(&self.command);
            cmd.args(&self.args)
                .current_dir(working_dir)
                .env("LOOM_GOAL_PROMPT", prompt)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());

            cmd.output().await
        })
        .await
        .map_err(|_| ToolError::Timeout)?;

        let output = result.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to run {}: {}", self.command, e))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "{} exited with {}: {}",
                self.command,
                output.status,
                stderr.trim()
            )));
        }

        Ok(TurnResult {
            reply: stdout,
            reasoning_content: None,
            tool_calls_summary: Vec::new(),
            usage: None,
        })
    }

    fn name(&self) -> &str {
        &self.command
    }
}

pub struct LoomTool {
    session_id: String,
    _working_dir: PathBuf,
    mcp_config_path: PathBuf,
    cancellation: Option<crate::cli_run::RunCancellation>,
    verbose: bool,
    model: Option<String>,
    provider: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    provider_type: Option<String>,
    agent: Option<String>,
    any_stream_event_sender: Option<Arc<dyn Fn(AnyStreamEvent) + Send + Sync>>,
}

impl LoomTool {
    pub fn new(
        session_id: String,
        working_dir: PathBuf,
        mcp_config_path: PathBuf,
    ) -> Self {
        Self {
            session_id,
            _working_dir: working_dir,
            mcp_config_path,
            cancellation: None,
            verbose: false,
            model: None,
            provider: None,
            base_url: None,
            api_key: None,
            provider_type: None,
            agent: None,
            any_stream_event_sender: None,
        }
    }

    pub fn with_cancellation(mut self, c: crate::cli_run::RunCancellation) -> Self {
        self.cancellation = Some(c);
        self
    }

    pub fn with_verbose(mut self, v: bool) -> Self {
        self.verbose = v;
        self
    }

    pub fn with_model(mut self, m: String) -> Self {
        self.model = Some(m);
        self
    }

    pub fn with_agent(mut self, a: String) -> Self {
        self.agent = Some(a);
        self
    }

    pub fn with_event_sender(
        mut self,
        sender: Arc<dyn Fn(AnyStreamEvent) + Send + Sync>,
    ) -> Self {
        self.any_stream_event_sender = Some(sender);
        self
    }
}

#[async_trait]
impl CodingTool for LoomTool {
    async fn execute(&self, prompt: &str, working_dir: &Path) -> Result<TurnResult, ToolError> {
        use crate::cli_run::{RunCmd, RunCompletion, RunOptions};
        use crate::message::UserContent;

        let on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>> =
            if let Some(ref sender) = self.any_stream_event_sender {
                let sender = sender.clone();
                Some(Box::new(move |ev: AnyStreamEvent| {
                    sender(ev);
                }))
            } else {
                Some(crate::stream_display::create_stdio_event_callback(
                    crate::stream_display::StreamDisplayConfig {
                        verbose: self.verbose,
                        display_max_len: 10000,
                        output_timestamp: false,
                        agent_display: None,
                    },
                ))
            };

        let opts = RunOptions {
            message: UserContent::Text(prompt.to_string()),
            working_folder: Some(working_dir.to_path_buf()),
            session_id: Some(self.session_id.clone()),
            agent: self.agent.clone(),
            verbose: self.verbose,
            got_adaptive: false,
            display_max_len: 10000,
            output_json: false,
            model: self.model.clone(),
            provider: self.provider.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            provider_type: self.provider_type.clone(),
            mcp_config_path: Some(self.mcp_config_path.clone()),
            cancellation: self.cancellation.clone(),
            thread_id: Some(self.session_id.clone()),
            output_timestamp: false,
            dry_run: false,
            any_stream_event_sender: self.any_stream_event_sender.clone(),
            bash_executor: None,
            extra_tools: None,
            acp_session_id: None,
            force_compact: false,
            chat_id: None,
        };

        let result = crate::cli_run::run_agent_with_options(&opts, &RunCmd::React, on_event)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("loom agent error: {}", e)))?;

        match result {
            RunCompletion::Finished(agent_result) => Ok(TurnResult {
                reply: agent_result.reply,
                reasoning_content: agent_result.reasoning_content,
                tool_calls_summary: Vec::new(),
                usage: None,
            }),
            RunCompletion::Cancelled => Err(ToolError::Aborted),
        }
    }

    fn name(&self) -> &str {
        "loom"
    }
}

pub fn generate_mcp_config(_tool_name: &str, db_path: &Path) -> String {
    let db_path_str = db_path.to_string_lossy().replace('\\', "\\\\");
    format!(
        r#"{{
  "mcpServers": {{
    "task": {{
      "command": "task-mcp-server",
      "args": ["--db-path", "{}"]
    }}
  }}
}}"#,
        db_path_str
    )
}
