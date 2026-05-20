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
    time_used_seconds: i64,
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
            time_used_seconds: 0,
        })
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
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

            let prompt = message::build_continuation_prompt(
                &self.objective,
                self.time_used_seconds,
            );

            tracing::info!(
                session_id = %self.task_id,
                iteration = self.iteration,
                tool = self.tool.name(),
                time_used_seconds = self.time_used_seconds,
                "executing goal turn"
            );

            match self.tool.execute(&prompt, &self.working_dir).await {
                Ok(_turn_result) => {
                    self.consecutive_failures = 0;
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
                Err(ToolError::ExecutionFailed(e)) => {
                    self.consecutive_failures += 1;
                    if self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        tracing::error!(
                            session_id = %self.task_id,
                            consecutive_failures = self.consecutive_failures,
                            iteration = self.iteration,
                            "consecutive failures limit reached"
                        );
                        self.cleanup().await;
                        return GoalOutcome::Error("consecutive tool failures".into());
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
            self.save_iteration_state().await;

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
        let mut meta = self.load_meta_async().await.unwrap_or_default();
        meta.iteration = self.iteration;
        meta.tool = self.tool.name().to_string();
        meta.time_used_seconds = self.time_used_seconds;

        meta.history.push(HistoryEntry {
            iteration: self.iteration,
            timestamp: Utc::now().to_rfc3339(),
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

    let tool: Box<dyn CodingTool> = resolve_tool(&meta.tool, db.path(), &working_dir)?;
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
        time_used_seconds: meta.time_used_seconds,
    })
}

fn resolve_tool(tool_name: &str, db_path: &std::path::Path, working_dir: &std::path::Path) -> Result<Box<dyn CodingTool>, GoalError> {
    match tool_name {
        "loom" => {
            let mcp_config_path = write_mcp_config(db_path, working_dir)?;
            Ok(Box::new(super::tool::LoomTool::new(
                "goal-session".to_string(),
                working_dir.to_path_buf(),
                mcp_config_path,
            )))
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
            )))
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
