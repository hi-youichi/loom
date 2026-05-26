use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use task_core::{CreateParams, TaskDb, TaskStatus};
use tokio::process::Child;
use tokio_util::sync::CancellationToken;
use tracing;

use super::message;
use super::state::{
    GoalError, GoalMeta, GoalOutcome, HistoryEntry, ToolError,
    DEFAULT_MAX_ITERATIONS, MAX_CONSECUTIVE_FAILURES, MAX_HISTORY_ENTRIES,
};
use super::tool::CodingTool;

/// Fraction of token budget remaining that triggers the budget-limit prompt.
const BUDGET_WARNING_FRACTION: f64 = 0.2;

pub struct GoalRunner {
    task_id: String,
    objective: String,
    db: Arc<TaskDb>,
    tool: Box<dyn CodingTool>,
    mcp_server: Option<Child>,
    working_dir: PathBuf,
    iteration: u32,
    max_iterations: u32,
    cancel: CancellationToken,
    consecutive_failures: u32,
    last_errors: Vec<String>,
    time_used_seconds: i64,
    /// Optional hard cap on cumulative token usage.
    token_budget: Option<u32>,
    /// Cumulative tokens consumed across all iterations.
    tokens_used: u32,
    /// Optional shell command to verify objective after each iteration.
    verify_command: Option<String>,
    /// Number of consecutive rate-limit retries in the current streak.
    rate_limit_retries: u32,
}

impl GoalRunner {
    pub async fn new(
        objective: String,
        working_dir: PathBuf,
        db: Arc<TaskDb>,
        tool: Box<dyn CodingTool>,
        cancel: CancellationToken,
    ) -> Result<Self, GoalError> {
        let task = db
            .create_task(&CreateParams {
                name: objective.clone(),
                description: objective.clone(),
                status: TaskStatus::InProgress,
                ..Default::default()
            })
            .await
            .map_err(|e| GoalError::Db(Box::new(e)))?;

        let mcp_server = spawn_mcp_server(&db).ok();

        Ok(Self {
            task_id: task.id,
            objective,
            db,
            tool,
            mcp_server,
            working_dir,
            iteration: 0,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            cancel,
            consecutive_failures: 0,
            last_errors: Vec::new(),
            time_used_seconds: 0,
            token_budget: None,
            tokens_used: 0,
            verify_command: None,
            rate_limit_retries: 0,
        })
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    /// Set the maximum number of iterations the goal loop will run.
    pub fn with_max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = max;
        self
    }

    /// Set an optional hard cap on cumulative token usage.
    pub fn with_token_budget(mut self, budget: u32) -> Self {
        self.token_budget = Some(budget);
        self
    }

    /// Set an optional verification command to run after each iteration.
    pub fn with_verify_command(mut self, cmd: String) -> Self {
        self.verify_command = Some(cmd);
        self
    }

    pub async fn run(&mut self) -> GoalOutcome {
        let start = Instant::now();
        loop {
            self.iteration += 1;

            if self.cancel.is_cancelled() {
                tracing::info!(session_id = %self.task_id, iteration = self.iteration, "goal cancelled");
                self.save_paused_state().await;
                self.cleanup().await;
                return GoalOutcome::Error("aborted by user".into());
            }

            if self.iteration > self.max_iterations {
                tracing::warn!(
                    session_id = %self.task_id,
                    "max iterations ({}) reached",
                    self.max_iterations
                );
                self.cleanup().await;
                return GoalOutcome::Error(format!(
                    "max iterations ({}) reached",
                    self.max_iterations
                ));
            }

            // Build prompt with optional budget warning and history summary.
            let history_summary = self.build_history_summary().await;
            let budget_warning = self.build_budget_warning();
            let prompt = message::build_continuation_prompt(
                &self.task_id,
                &self.objective,
                self.time_used_seconds,
                self.tokens_used,
                self.token_budget,
                &history_summary,
                &budget_warning,
                self.verify_command.as_deref(),
            );

            tracing::info!(
                session_id = %self.task_id,
                iteration = self.iteration,
                tool = self.tool.name(),
                time_used_seconds = self.time_used_seconds,
                "executing goal turn"
            );

            eprintln!("\n{}",
                crate::stream_display::panel_format::format_panel_line(
                    "GOAL",
                    &format!("iteration {} | tool: {} | time: {}s",
                        self.iteration, self.tool.name(), self.time_used_seconds),
                )
            );

            let mut work_summary: Option<String> = None;

            match self.tool.execute(&prompt, &self.working_dir).await {
                Ok(turn_result) => {
                    self.consecutive_failures = 0;
                    self.last_errors.clear();
                    self.rate_limit_retries = 0;

                    // Accumulate token usage.
                    if let Some(ref usage) = turn_result.usage {
                        self.tokens_used += usage.total_tokens;
                    }

                    // Store work summary for history injection.
                    work_summary = turn_result.work_summary.clone();
                    if let Some(ref reasoning) = turn_result.reasoning_content {
                        if !reasoning.trim().is_empty() {
                            eprintln!("{}",
                                crate::stream_display::panel_format::format_panel_line(
                                    "THINKING", &crate::stream_display::render_markdown(reasoning).to_string()
                                )
                            );
                        }
                    }
                    if !turn_result.reply.trim().is_empty() {
                        eprintln!("{}", crate::stream_display::render_markdown(&turn_result.reply));
                    }
                    for tc in &turn_result.tool_calls_summary {
                        eprintln!("{}",
                            crate::stream_display::panel_format::format_panel_line(
                                "TOOL", &format!("{} → {}", tc.tool_name, tc.result_preview)
                            )
                        );
                    }
                }
                Err(ToolError::Aborted) => {
                    tracing::info!(session_id = %self.task_id, iteration = self.iteration, "tool aborted");
                    self.save_paused_state().await;
                    self.cleanup().await;
                    return GoalOutcome::Error("aborted by user".into());
                }
                Err(ToolError::Timeout) => {
                    tracing::warn!(session_id = %self.task_id, iteration = self.iteration, "tool timeout");
                    self.save_iteration_state().await;
                    continue;
                }
                Err(ToolError::RateLimited(msg)) => {
                    self.rate_limit_retries += 1;
                    let backoff_secs = 2u64.pow(self.rate_limit_retries.min(5));
                    let max_rate_limit_retries: u32 = 6;
                    if self.rate_limit_retries > max_rate_limit_retries {
                        tracing::error!(
                            session_id = %self.task_id,
                            retries = self.rate_limit_retries,
                            "rate-limit retries exhausted"
                        );
                        self.cleanup().await;
                        return GoalOutcome::Error(format!(
                            "API rate-limited after {} retries: {}",
                            self.rate_limit_retries, msg
                        ));
                    }
                    tracing::warn!(
                        session_id = %self.task_id,
                        iteration = self.iteration,
                        retry = self.rate_limit_retries,
                        backoff_secs = backoff_secs,
                        "API rate-limited, backing off"
                    );
                    eprintln!("\n  ⚠ API rate-limited, retrying in {}s (attempt {}/{}) …",
                        backoff_secs, self.rate_limit_retries, max_rate_limit_retries);
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)) => {}
                        _ = self.cancel.cancelled() => {
                            self.save_paused_state().await;
                            self.cleanup().await;
                            return GoalOutcome::Error("aborted by user".into());
                        }
                    }
                    // Don't count rate-limit as a consecutive failure — retry the same iteration.
                    self.iteration -= 1;
                    self.save_iteration_state().await;
                    continue;
                }
                Err(ToolError::ExecutionFailed(e)) => {
                    self.consecutive_failures += 1;
                    self.last_errors.push(e.clone());
                    if self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        tracing::error!(
                            session_id = %self.task_id,
                            consecutive_failures = self.consecutive_failures,
                            iteration = self.iteration,
                            "consecutive failures limit reached"
                        );
                        self.cleanup().await;
                        let details = self.last_errors.join("\n");
                        return GoalOutcome::Error(format!("consecutive tool failures:\n{}", details));
                    }
                    tracing::error!(
                        session_id = %self.task_id,
                        iteration = self.iteration,
                        error = %e,
                        "tool failed"
                    );
                    self.save_iteration_state().await;
                    continue;
                }
            }

            self.time_used_seconds = start.elapsed().as_secs() as i64;
            self.save_iteration_state_with_summary(work_summary.as_deref()).await;

            // Check token budget exhaustion.
            if let Some(budget) = self.token_budget {
                if self.tokens_used >= budget {
                    tracing::warn!(
                        session_id = %self.task_id,
                        tokens_used = self.tokens_used,
                        budget = budget,
                        "token budget exhausted"
                    );
                    self.cleanup().await;
                    return GoalOutcome::UsageLimited {
                        tokens_used: self.tokens_used,
                        token_budget: budget,
                    };
                }
            }

            // Run verify command if configured.
            if let Some(ref verify_cmd) = self.verify_command {
                let verify_passed = self.run_verify_command(verify_cmd).await;
                if verify_passed {
                    tracing::info!(session_id = %self.task_id, "verify command passed");
                    // Auto-mark complete.
                    if let Err(e) = self.db.atomic_update_status(
                        &self.task_id, TaskStatus::InProgress, TaskStatus::Completed,
                    ).await {
                        tracing::error!(session_id = %self.task_id, error = %e, "failed to mark complete after verify");
                    }
                    self.cleanup().await;
                    return GoalOutcome::Achieved;
                }
            }

            let task = match self.db.show_task(&self.task_id).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!(session_id = %self.task_id, error = %e, "failed to read task");
                    self.cleanup().await;
                    return GoalOutcome::Error(format!("db error: {}", e));
                }
            };
            if task.status == TaskStatus::Completed {
                tracing::info!(
                    session_id = %self.task_id,
                    iterations = self.iteration,
                    time_used_seconds = self.time_used_seconds,
                    "goal achieved"
                );
                self.cleanup().await;
                return GoalOutcome::Achieved;
            }
        }
    }

    async fn save_iteration_state(&self) {
        self.save_iteration_state_with_summary(None).await;
    }

    async fn save_iteration_state_with_summary(&self, summary: Option<&str>) {
        let mut meta = self.load_meta_async().await.unwrap_or_default();
        meta.iteration = self.iteration;
        meta.tool = self.tool.name().to_string();
        meta.time_used_seconds = self.time_used_seconds;
        meta.token_budget = self.token_budget;
        meta.tokens_used = self.tokens_used;

        meta.history.push(HistoryEntry {
            iteration: self.iteration,
            timestamp: Utc::now().to_rfc3339(),
            summary: summary.map(|s| s.to_string()),
        });
        if meta.history.len() > MAX_HISTORY_ENTRIES {
            let start = meta.history.len() - MAX_HISTORY_ENTRIES;
            meta.history = meta.history.split_off(start);
        }

        if let Err(e) = self.save_meta_async(&meta).await {
            tracing::error!(session_id = %self.task_id, error = %e, "failed to save iteration state");
        }
    }

    async fn save_paused_state(&self) {
        if let Err(e) = self.db.atomic_update_status(
            &self.task_id,
            TaskStatus::InProgress,
            TaskStatus::Pending,
        ).await {
            tracing::error!(session_id = %self.task_id, error = %e, "failed to set task to paused");
        }
        self.save_iteration_state().await;
    }

    /// Build a short summary of recent iteration history for prompt injection.
    async fn build_history_summary(&self) -> Option<String> {
        let meta = self.load_meta_async().await.ok()?;
        if meta.history.is_empty() {
            return None;
        }
        let lines: Vec<String> = meta.history.iter().map(|h| {
            match &h.summary {
                Some(s) => format!("  iter {}: {}", h.iteration, s),
                None => format!("  iter {}: completed", h.iteration),
            }
        }).collect();
        Some(format!("Previous iterations:\n{}", lines.join("\n")))
    }

    /// Build budget warning text if close to limit.
    fn build_budget_warning(&self) -> Option<String> {
        let budget = self.token_budget?;
        if budget == 0 { return None; }
        let remaining = budget.saturating_sub(self.tokens_used);
        let fraction = remaining as f64 / budget as f64;
        if fraction <= BUDGET_WARNING_FRACTION {
            Some(format!(
                "WARNING: Token budget almost exhausted. {}/{} tokens used ({}% remaining). \
                 Prioritize wrapping up or calling task_update(status='completed').",
                self.tokens_used, budget, (fraction * 100.0) as u32
            ))
        } else {
            None
        }
    }

    /// Run the verify command and return true if it succeeds (exit code 0).
    async fn run_verify_command(&self, cmd: &str) -> bool {
        tracing::info!(session_id = %self.task_id, cmd = cmd, "running verify command");
        eprintln!("{}",
            crate::stream_display::panel_format::format_panel_line(
                "VERIFY", cmd,
            )
        );
        // Use cmd.exe on Windows, sh elsewhere.
        let result = if cfg!(windows) {
            tokio::process::Command::new("cmd")
                .args(["/C", cmd])
                .current_dir(&self.working_dir)
                .output()
                .await
        } else {
            tokio::process::Command::new("sh")
                .args(["-c", cmd])
                .current_dir(&self.working_dir)
                .output()
                .await
        };
        match result {
            Ok(output) => {
                if output.status.success() {
                    eprintln!("{}",
                        crate::stream_display::panel_format::format_panel_line(
                            "VERIFY", "passed",
                        )
                    );
                    true
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!("{}",
                        crate::stream_display::panel_format::format_panel_line(
                            "VERIFY", &format!("failed: {}", stderr.trim()),
                        )
                    );
                    false
                }
            }
            Err(e) => {
                tracing::error!(session_id = %self.task_id, error = %e, "verify command failed to execute");
                eprintln!("{}",
                    crate::stream_display::panel_format::format_panel_line(
                        "VERIFY", &format!("error: {}", e),
                    )
                );
                false
            }
        }
    }

    async fn load_meta_async(&self) -> Result<GoalMeta, GoalError> {
        let val = self.db.get_meta(&self.task_id, "goal")
            .await
            .map_err(|e| GoalError::Db(e))?;
        match val {
            Some(v) => serde_json::from_value(v).map_err(|e| GoalError::Db(Box::new(e))),
            None => Ok(GoalMeta::default()),
        }
    }

    async fn save_meta_async(&self, meta: &GoalMeta) -> Result<(), GoalError> {
        let val = serde_json::to_value(meta).map_err(|e| GoalError::Db(Box::new(e)))?;
        self.db.set_meta(&self.task_id, "goal", &val)
            .await
            .map_err(|e| GoalError::Db(e))
    }

    async fn cleanup(&mut self) {
        if let Some(ref mut child) = self.mcp_server {
            let _ = child.kill().await;
        }
    }
}

fn spawn_mcp_server(db: &TaskDb) -> Result<Child, GoalError> {
    let db_path = db.path();
    let child = tokio::process::Command::new("task-mcp-server")
        .arg("--db-path")
        .arg(db_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(GoalError::Io)?;
    Ok(child)
}

pub async fn resume(
    id: &str,
    working_dir: PathBuf,
    db: Arc<TaskDb>,
    cancel: CancellationToken,
    run_cancellation: Option<crate::cli_run::RunCancellation>,
) -> Result<GoalRunner, GoalError> {
    resume_with_event_sender(id, working_dir, db, cancel, run_cancellation, None).await
}

pub async fn resume_with_event_sender(
    id: &str,
    working_dir: PathBuf,
    db: Arc<TaskDb>,
    cancel: CancellationToken,
    run_cancellation: Option<crate::cli_run::RunCancellation>,
    event_sender: Option<Arc<dyn Fn(crate::cli_run::AnyStreamEvent) + Send + Sync>>,
) -> Result<GoalRunner, GoalError> {
    let updated = db
        .atomic_update_status(id, TaskStatus::Pending, TaskStatus::InProgress)
        .await
        .map_err(|e| GoalError::Db(Box::new(e)))?;
    if !updated {
        let task = db.show_task(id).await.map_err(|e| GoalError::Db(Box::new(e)))?;
        return Err(GoalError::Resume(format!(
            "task {} is not paused (status: {})",
            id, task.status
        )));
    }

    let task = db.show_task(id).await.map_err(|e| GoalError::Db(Box::new(e)))?;
    let meta_val = db
        .get_meta(id, "goal")
        .await
        .map_err(|e| GoalError::Db(e))?;
    let meta: GoalMeta = match meta_val {
        Some(v) => serde_json::from_value(v).map_err(|e| GoalError::Db(Box::new(e)))?,
        None => GoalMeta::default(),
    };

    let tool: Box<dyn CodingTool> = resolve_tool(&meta.tool, id, db.path(), &working_dir, &run_cancellation, &event_sender, &cancel)?;
    let mcp_server = spawn_mcp_server(&db).ok();

    Ok(GoalRunner {
        task_id: task.id,
        objective: task.description,
        db,
        tool,
        mcp_server,
        working_dir,
        iteration: meta.iteration,
        max_iterations: DEFAULT_MAX_ITERATIONS,
        cancel,
        consecutive_failures: 0,
        last_errors: Vec::new(),
        time_used_seconds: meta.time_used_seconds,
        token_budget: meta.token_budget,
        tokens_used: meta.tokens_used,
        verify_command: meta.verify_command,
        rate_limit_retries: 0,
    })
}

fn resolve_tool(
    tool_name: &str,
    id: &str,
    db_path: &std::path::Path,
    working_dir: &std::path::Path,
    run_cancellation: &Option<crate::cli_run::RunCancellation>,
    event_sender: &Option<Arc<dyn Fn(crate::cli_run::AnyStreamEvent) + Send + Sync>>,
    cancel: &CancellationToken,
) -> Result<Box<dyn CodingTool>, GoalError> {
    match tool_name {
"loom" => {
            let mcp_config_path = write_mcp_config(db_path, working_dir)?;
            let session_id = format!("goal-{}", &id[..8]);
            let mut tool = super::tool::LoomTool::new(
                session_id,
                working_dir.to_path_buf(),
                mcp_config_path,
            );
            if let Some(ref rc) = run_cancellation {
                tool = tool.with_cancellation(rc.clone());
            }
            if let Some(ref sender) = event_sender {
                tool = tool.with_event_sender(sender.clone());
            }
            Ok(Box::new(tool))
        }
        name => {
            let args = match name {
                "codex" => vec!["--goal-prompt".to_string()],
                "claude" => vec!["--goal-prompt".to_string()],
                "cursor" => vec!["--goal-prompt".to_string()],
                _ => vec![],
            };
            Ok(Box::new(super::tool::ShellTool::new(
                name.to_string(),
                args,
            ).with_cancel(cancel.clone())))
        }
    }
}

fn write_mcp_config(db_path: &std::path::Path, working_dir: &std::path::Path) -> Result<std::path::PathBuf, GoalError> {
    let config_content = super::tool::generate_mcp_config("task", db_path);
    let config_path = working_dir.join(".loom").join("goal-mcp.json");
    std::fs::create_dir_all(config_path.parent().unwrap())?;
    std::fs::write(&config_path, config_content)?;
    Ok(config_path)
}
