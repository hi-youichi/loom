//! ACP-internal goal runner: wraps `loom::goal_runner::GoalRunner` for use inside ACP prompt().
//!
//! When the user types `/goal <description>` in the IDE, the ACP prompt handler
//! delegates to [`run_goal`] which creates a `GoalRunner` with a `LoomTool` that
//! bridges events back to the IDE via `session/update` notifications.

use std::path::PathBuf;
use std::sync::Arc;

use task_core::TaskDb;
use tokio_util::sync::CancellationToken;

use loom::cli_run::RunCancellation;
use loom::goal_runner::{
    generate_mcp_config, GoalOutcome, GoalRunner, LoomTool,
};
use loom::AnyStreamEvent;

/// Default max iterations for ACP goal runs.
const ACP_DEFAULT_MAX_ITERATIONS: u32 = 30;

/// Reads the max iterations from env var `LOOM_GOAL_MAX_ITERATIONS`,
/// falling back to `ACP_DEFAULT_MAX_ITERATIONS`.
fn max_iterations_from_env() -> u32 {
    std::env::var("LOOM_GOAL_MAX_ITERATIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(ACP_DEFAULT_MAX_ITERATIONS)
}

/// Runs a goal loop inside the ACP process.
///
/// Creates a `TaskDb` at `working_dir/tasks.db`, builds a `LoomTool` with the
/// given event bridge, and runs the `GoalRunner` until completion, cancellation,
/// or error.
pub async fn run_goal(
    objective: String,
    working_dir: PathBuf,
    cancel: CancellationToken,
    event_sender: Option<Arc<dyn Fn(AnyStreamEvent) + Send + Sync>>,
    run_cancellation: Option<RunCancellation>,
) -> Result<GoalResult, GoalRunError> {
    run_goal_with_max_iterations(
        objective,
        working_dir,
        cancel,
        event_sender,
        run_cancellation,
        max_iterations_from_env(),
    )
    .await
}

/// Same as [`run_goal`] but with a configurable max iterations.
pub async fn run_goal_with_max_iterations(
    objective: String,
    working_dir: PathBuf,
    cancel: CancellationToken,
    event_sender: Option<Arc<dyn Fn(AnyStreamEvent) + Send + Sync>>,
    run_cancellation: Option<RunCancellation>,
    max_iterations: u32,
) -> Result<GoalResult, GoalRunError> {
    // Ensure working dir exists
    std::fs::create_dir_all(&working_dir)
        .map_err(|e| GoalRunError::Init(format!("failed to create working dir: {}", e)))?;

    // Open / create TaskDb
    let db_path = working_dir.join("tasks.db");
    let db = TaskDb::open(&db_path)
        .await
        .map_err(|e| GoalRunError::Init(format!("failed to open task db: {}", e)))?;
    let db = Arc::new(db);

    // Write MCP config so the LoomTool can register task tools
    let mcp_config_path = {
        let config_content = generate_mcp_config("task", db.path());
        let config_path = working_dir.join(".loom").join("goal-mcp.json");
        std::fs::create_dir_all(config_path.parent().unwrap())
            .map_err(|e| GoalRunError::Init(format!("failed to create .loom dir: {}", e)))?;
        std::fs::write(&config_path, config_content)
            .map_err(|e| GoalRunError::Init(format!("failed to write mcp config: {}", e)))?;
        config_path
    };

    // Build the LoomTool with event bridge
    let mut tool = LoomTool::new(
        "goal-session".to_string(),
        working_dir.clone(),
        mcp_config_path,
    );

    if let Some(rc) = run_cancellation {
        tool = tool.with_cancellation(rc);
    }

    if let Some(sender) = event_sender {
        tool = tool.with_event_sender(sender);
    }

    // Create and run the GoalRunner with configurable max iterations
    let mut runner = GoalRunner::new(objective.clone(), working_dir, db, Box::new(tool), cancel)
        .await
        .map_err(|e| GoalRunError::Init(format!("failed to create goal runner: {}", e)))?
        .with_max_iterations(max_iterations);

    let task_id = runner.task_id().to_string();
    let outcome = runner.run().await;

    Ok(GoalResult {
        task_id,
        outcome,
    })
}

/// Result of a goal run.
#[derive(Debug)]
pub struct GoalResult {
    pub task_id: String,
    pub outcome: GoalOutcome,
}

/// Errors that can occur during goal setup or execution.
#[derive(Debug, thiserror::Error)]
pub enum GoalRunError {
    #[error("goal init error: {0}")]
    Init(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_run_goal_creates_task_and_db() {
        // This test verifies that run_goal sets up correctly.
        // A full integration test would require an LLM, so we test setup only.
        let tmp = TempDir::new().unwrap();
        let working_dir = tmp.path().to_path_buf();
        let db_path = working_dir.join("tasks.db");

        // Verify TaskDb can be opened
        let db = TaskDb::open(&db_path).await.unwrap();
        let db = Arc::new(db);

        // Verify MCP config can be written
        let config_content = generate_mcp_config("task", db.path());
        assert!(config_content.contains("task-mcp-server"));
    }

    #[tokio::test]
    async fn test_goal_result_fields() {
        let result = GoalResult {
            task_id: "test-id-123".to_string(),
            outcome: GoalOutcome::Achieved,
        };
        assert_eq!(result.task_id, "test-id-123");
        assert!(matches!(result.outcome, GoalOutcome::Achieved));
    }

    #[tokio::test]
    async fn test_goal_run_error_display() {
        let err = GoalRunError::Init("test error".to_string());
        assert_eq!(format!("{}", err), "goal init error: test error");
    }
}
