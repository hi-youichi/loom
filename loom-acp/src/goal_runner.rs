//! ACP-internal goal runner: wraps `loom::goal_runner::GoalRunner` for use inside ACP prompt().
//!
//! When the user types `/goal <description>` in the IDE, the ACP prompt handler
//! delegates to [`run_goal`] which creates a `GoalRunner` with a `LoomTool` that
//! bridges events back to the IDE via `session/update` notifications.
//!
//! **TODO**: GoalRunner/LoomTool/generate_mcp_config were moved to loom-agent.
//! This module needs to be updated to use the loom-agent equivalents.

use std::path::PathBuf;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use loom_cli_types::RunCancellation;
use loom_cli_types::goal_runner::GoalOutcome;

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
    #[error("goal runner unavailable: {0}")]
    Unavailable(String),
}

/// Runs a goal loop inside the ACP process.
///
/// **TODO**: GoalRunner has been moved to loom-agent crate.
/// This function currently returns an error indicating the feature is unavailable.
pub async fn run_goal(
    _objective: String,
    _working_dir: PathBuf,
    _cancel: CancellationToken,
    _event_sender: Option<Arc<dyn Fn(loom_agent::AnyStreamEvent) + Send + Sync>>,
    _run_cancellation: Option<RunCancellation>,
) -> Result<GoalResult, GoalRunError> {
    Err(GoalRunError::Unavailable(
        "GoalRunner has been moved to loom-agent crate and is not yet bridged".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
