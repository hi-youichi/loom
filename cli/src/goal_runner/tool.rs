use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use loom::agent_run::{run_agent_with_options, AnyStreamEvent as FullAnyStreamEvent};
use loom_cli_types::AnyStreamEvent;
use loom_stream::StreamEvent;

use loom_cli_types::goal_runner::state::{ToolCallSummary, ToolError, TurnResult};

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
    cancel: Option<CancellationToken>,
}

impl ShellTool {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self {
            command,
            args,
            timeout: Duration::from_secs(DEFAULT_TOOL_TIMEOUT_SECS),
            cancel: None,
        }
    }
    #[allow(dead_code)]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
       }

    pub fn with_cancel(mut self, cancel: CancellationToken) -> Self {
        self.cancel = Some(cancel);
        self
    }
}

#[async_trait]
impl CodingTool for ShellTool {
    async fn execute(&self, prompt: &str, working_dir: &Path) -> Result<TurnResult, ToolError> {
        let mut cmd = tokio::process::Command::new(&self.command);
        cmd.args(&self.args)
            .current_dir(working_dir)
            .env("LOOM_GOAL_PROMPT", prompt)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to spawn {}: {}", self.command, e))
        })?;

        let output_fut = async {
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            let status = child.wait().await?;
            let mut stdout_buf = Vec::new();
            let mut stderr_buf = Vec::new();
            if let Some(mut out) = stdout {
                use tokio::io::AsyncReadExt;
                let _ = out.read_to_end(&mut stdout_buf).await;
            }
            if let Some(mut err) = stderr {
                use tokio::io::AsyncReadExt;
                let _ = err.read_to_end(&mut stderr_buf).await;
            }
            Ok::<_, std::io::Error>((status, stdout_buf, stderr_buf))
        };

        let result = if let Some(ref cancel) = self.cancel {
            tokio::select! {
                res = output_fut => res,
                _ = cancel.cancelled() => {
                    let _ = child.kill().await;
                    return Err(ToolError::Aborted);
                }
            }
        } else {
            match tokio::time::timeout(self.timeout, output_fut).await {
                Ok(res) => res,
                Err(_) => {
                    let _ = child.kill().await;
                    return Err(ToolError::Timeout);
                }
            }
        }.map_err(|e| ToolError::ExecutionFailed(format!("{} failed: {}", self.command, e)))?;

        let (status, stdout_buf, stderr_buf) = result;

        if let Some(ref cancel) = self.cancel {
            if cancel.is_cancelled() {
                return Err(ToolError::Aborted);
            }
        }

        let stdout = String::from_utf8_lossy(&stdout_buf).to_string();
        let stderr = String::from_utf8_lossy(&stderr_buf).to_string();

        if !status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "{} exited with {}: {}",
                self.command,
                status,
                stderr.trim()
            )));
        }

        Ok(TurnResult {
            reply: stdout,
            reasoning_content: None,
            tool_calls_summary: Vec::new(),
            usage: None,
            work_summary: None,
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
    cancellation: Option<loom_cli_types::RunCancellation>,
    verbose: bool,
    model: Option<String>,
    provider: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    provider_type: Option<String>,
    agent: Option<String>,
    any_stream_event_sender: Option<Arc<dyn Fn(FullAnyStreamEvent) + Send + Sync>>,
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

    pub fn with_cancellation(mut self, c: loom_cli_types::RunCancellation) -> Self {
        self.cancellation = Some(c);
        self
    }
    #[allow(dead_code)]
    pub fn with_verbose(mut self, v: bool) -> Self {
        self.verbose = v;
        self
    }

    pub fn with_model(mut self, m: String) -> Self {
        self.model = Some(m);
        self
    }
    #[allow(dead_code)]
    pub fn with_agent(mut self, a: String) -> Self {
        self.agent = Some(a);
        self
    }

    pub fn with_event_sender(
        mut self,
        sender: Arc<dyn Fn(FullAnyStreamEvent) + Send + Sync>,
    ) -> Self {
        self.any_stream_event_sender = Some(sender);
        self
    }
}

#[async_trait]
impl CodingTool for LoomTool {
    async fn execute(&self, prompt: &str, working_dir: &Path) -> Result<TurnResult, ToolError> {
        use loom::agent_run::{RunCmd, RunCompletion, RunOptions};
        use loom_llm::message::UserContent;

        let tool_summaries: Arc<Mutex<Vec<ToolCallSummary>>> =
            Arc::new(Mutex::new(Vec::new()));

        let on_event: Option<Box<dyn FnMut(FullAnyStreamEvent) + Send>> =
            if let Some(ref sender) = self.any_stream_event_sender {
                let sender = sender.clone();
                let summaries = tool_summaries.clone();
                Some(Box::new(move |ev: FullAnyStreamEvent| {
                    // Convert full event to cli_types event for summary collection
                    if let Some(cli_ev) = loom::agent_run::to_loom_any_stream_event(&ev) {
                        collect_tool_summary(&cli_ev, &summaries);
                    }
                    sender(ev);
                }))
            } else {
                let mut original = loom_stream_display::create_stdio_event_callback(
                    loom_stream_display::StreamDisplayConfig {
                        verbose: self.verbose,
                        display_max_len: 10000,
                        output_timestamp: false,
                        agent_display: None,
                        use_spinner: true,
                    },
                );
                let summaries = tool_summaries.clone();
                Some(Box::new(move |ev: FullAnyStreamEvent| {
                    if let Some(cli_ev) = loom::agent_run::to_loom_any_stream_event(&ev) {
                        collect_tool_summary(&cli_ev, &summaries);
                        original(cli_ev);
                    }
                }))
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
            debug_llm: false,
            any_stream_event_sender: self.any_stream_event_sender.as_ref().map(|sender| {
                let sender = sender.clone();
                Arc::new(move |ev: loom_cli_types::AnyStreamEvent| {
                    // Convert cli_types event to full event (best-effort, only React supported)
                    let full_ev = FullAnyStreamEvent::from_loom(ev);
                    sender(full_ev);
                }) as Arc<dyn Fn(loom_cli_types::AnyStreamEvent) + Send + Sync>
            }),
            bash_executor: None,
            extra_tools: None,
            acp_session_id: None,
            force_compact: false,
            chat_id: None,
            worktree: false,
            goal_mode: true,
        };

        let result = run_agent_with_options(&opts, &RunCmd::React, on_event)
            .await
            .map_err(|e| {
                let msg = format!("loom agent error: {}", e);
                if ToolError::is_transient_api_error(&msg) {
                    ToolError::RateLimited(msg)
                } else {
                    ToolError::ExecutionFailed(msg)
                }
            })?;

        let tool_calls_summary = tool_summaries.lock().unwrap().drain(..).collect();

        match result {
            RunCompletion::Finished(agent_result) => Ok(TurnResult {
                reply: agent_result.reply,
                reasoning_content: agent_result.reasoning_content,
                tool_calls_summary,
                usage: None,
                work_summary: None,
            }),
            RunCompletion::Cancelled => Err(ToolError::Aborted),
            RunCompletion::Error(e) => Err(ToolError::ExecutionFailed(format!("agent error: {}", e.0))),
        }
    }

    fn name(&self) -> &str {
        "loom"
    }
}

pub fn generate_mcp_config(_tool_name: &str, db_path: &Path) -> String {
    let db_path_str = db_path.to_string_lossy().replace('\\', "\\\\");
    format!(
        r#"{{"mcpServers":{{"task":{{"command":"task-mcp-server","args":["--db-path","{}"]}}}}}}"#,
        db_path_str
    )
}

/// Returns default CLI arguments for known shell-based coding tools.
pub fn shell_tool_args(tool_name: &str) -> Vec<String> {
    match tool_name {
        "codex" | "claude" | "cursor" => vec!["--goal-prompt".to_string()],
        _ => vec![],
    }
}

/// Extract tool name + result preview from `StreamEvent::ToolEnd` events
/// into a shared summary list for the goal runner to display after each turn.
fn collect_tool_summary(
    ev: &AnyStreamEvent,
    summaries: &Arc<Mutex<Vec<ToolCallSummary>>>,
) {
    if let AnyStreamEvent::React(StreamEvent::ToolEnd { name, result, .. }) = ev {
        let preview = loom_stream_display::tool_summary::truncate(
            result.lines().next().unwrap_or(result),
            80,
        );
        summaries.lock().unwrap().push(ToolCallSummary {
            tool_name: name.clone(),
            result_preview: preview.to_string(),
        });
    }
}
