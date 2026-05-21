#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::NamedTempFile;
    use tokio_util::sync::CancellationToken;

    use task_core::{CreateParams, TaskDb, TaskStatus};

    use crate::goal_runner::{
        self, CodingTool, GoalMeta, GoalOutcome, GoalRunner, HistoryEntry,
        ShellTool, ToolError, TurnResult, MAX_HISTORY_ENTRIES,
    };

    async fn test_db() -> (TaskDb, NamedTempFile) {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_path_buf();
        let db = TaskDb::open(&path).await.unwrap();
        (db, f)
    }

    #[tokio::test]
    async fn test_save_iteration_state_history_limit() {
        let (db, _f) = test_db().await;
        let db = Arc::new(db);
        let task = db.create_task(&CreateParams {
            name: "history-test".into(),
            ..Default::default()
        }).await.unwrap();

        for i in 0..25 {
            let mut meta: GoalMeta = match db.get_meta(&task.id, "goal").await.unwrap() {
                Some(v) => serde_json::from_value(v).unwrap(),
                None => GoalMeta::default(),
            };
            meta.iteration = i + 1;
            meta.history.push(HistoryEntry {
                iteration: i + 1,
                timestamp: format!("2025-01-01T00:{:02}:00Z", i),
            });
            if meta.history.len() > MAX_HISTORY_ENTRIES {
                let start = meta.history.len() - MAX_HISTORY_ENTRIES;
                meta.history = meta.history.split_off(start);
            }
            let val = serde_json::to_value(&meta).unwrap();
            db.set_meta(&task.id, "goal", &val).await.unwrap();
        }

        let meta_val = db.get_meta(&task.id, "goal").await.unwrap().unwrap();
        let meta: GoalMeta = serde_json::from_value(meta_val).unwrap();
        assert_eq!(meta.history.len(), MAX_HISTORY_ENTRIES);
        assert_eq!(meta.history[0].iteration, 6);
        assert_eq!(meta.history[19].iteration, 25);
    }

    #[tokio::test]
    async fn test_resume_rejects_non_paused_task() {
        let (db, _f) = test_db().await;
        let db = Arc::new(db);
        let task = db.create_task(&CreateParams {
            name: "resume-test".into(),
            status: TaskStatus::InProgress,
            ..Default::default()
        }).await.unwrap();

        let working_dir = std::env::temp_dir();
        let cancel = CancellationToken::new();

        let result = goal_runner::resume(
            &task.id,
            working_dir,
            db,
            cancel,
            None,
        ).await;

        assert!(result.is_err());
        if let Err(err) = result {
            assert!(err.to_string().contains("not paused"));
        }
    }

    #[tokio::test]
    async fn test_atomic_update_status_concurrent_rejection() {
        let (db, _f) = test_db().await;
        let db = Arc::new(db);
        let task = db.create_task(&CreateParams {
            name: "concurrent".into(),
            status: TaskStatus::Pending,
            ..Default::default()
        }).await.unwrap();

        let ok1 = db.atomic_update_status(&task.id, TaskStatus::Pending, TaskStatus::InProgress).await.unwrap();
        assert!(ok1);

        let ok2 = db.atomic_update_status(&task.id, TaskStatus::Pending, TaskStatus::InProgress).await.unwrap();
        assert!(!ok2);
    }

    #[tokio::test]
    async fn test_goal_runner_creates_task() {
        let (db, _f) = test_db().await;
        let db = Arc::new(db);
        let cancel = CancellationToken::new();

        let tool = Box::new(ShellTool::new(
            "echo".to_string(),
            vec![],
        ));

        let runner = GoalRunner::new(
            "test objective".to_string(),
            std::env::temp_dir(),
            db.clone(),
            tool,
            cancel,
        ).await.unwrap();

        let task_id = runner.task_id().to_string();
        let task = db.show_task(&task_id).await.unwrap();
        assert_eq!(task.name, "test objective");
        assert_eq!(task.status, TaskStatus::InProgress);
    }

    #[tokio::test]
    async fn test_consecutive_failures_error() {
        let (db, _f) = test_db().await;
        let db = Arc::new(db);
        let cancel = CancellationToken::new();

        let tool = Box::new(FailTool);
        let mut runner = GoalRunner::new(
            "test".to_string(),
            std::env::temp_dir(),
            db.clone(),
            tool,
            cancel,
        ).await.unwrap();

        let outcome = runner.run().await;
        match outcome {
            GoalOutcome::Error(e) => {
                assert!(e.contains("consecutive tool failures"), "got: {}", e);
            }
            _ => panic!("expected error, got: {:?}", outcome),
        }
    }

    struct FailTool;

    #[async_trait::async_trait]
    impl CodingTool for FailTool {
        async fn execute(
            &self,
            _prompt: &str,
            _working_dir: &std::path::Path,
        ) -> Result<TurnResult, ToolError> {
            Err(ToolError::ExecutionFailed("always fails".into()))
        }
        fn name(&self) -> &str {
            "fail"
        }
    }
}
