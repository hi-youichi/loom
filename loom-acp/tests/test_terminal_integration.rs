#[cfg(test)]
mod tests {
    use loom_acp::terminal::{TerminalError, TerminalManager, TerminalStatus};

    #[tokio::test]
    async fn test_terminal_create_and_output() {
        let manager = TerminalManager::new();

        let term_id = manager
            .create_terminal(
                "cmd".to_string(),
                vec!["/C".to_string(), "echo hello".to_string()],
                None,
                vec![],
                None,
            )
            .await
            .unwrap();

        assert!(term_id.starts_with("term-"));

        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let result = manager.get_output(&term_id).await;
        assert!(result.is_some());
        let (output, _truncated, status) = result.unwrap();
        assert!(output.contains("hello"), "Expected 'hello' in output, got: {}", output);

        if let Some(TerminalStatus::Completed { exit_code, .. }) = status {
            assert_eq!(exit_code, Some(0));
        }
    }

    #[tokio::test]
    async fn test_terminal_kill() {
        let manager = TerminalManager::new();

        let term_id = manager
            .create_terminal(
                "cmd".to_string(),
                vec!["/C".to_string(), "ping -n 60 127.0.0.1".to_string()],
                None,
                vec![],
                None,
            )
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let _status_before = manager.get_status(&term_id).await;

        manager.kill(&term_id).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let status = manager.get_status(&term_id).await;
        assert_eq!(status, Some(TerminalStatus::Killed));
    }

    #[tokio::test]
    async fn test_terminal_release() {
        let manager = TerminalManager::new();

        let term_id = manager
            .create_terminal(
                "cmd".to_string(),
                vec!["/C".to_string(), "ping -n 60 127.0.0.1".to_string()],
                None,
                vec![],
                None,
            )
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        manager.release(&term_id).await.unwrap();

        let status = manager.get_status(&term_id).await;
        assert_eq!(status, Some(TerminalStatus::Released));

        let result = manager.kill(&term_id).await;
        assert!(matches!(result, Err(TerminalError::AlreadyReleased(_))));
    }

    #[tokio::test]
    async fn test_terminal_wait_for_exit() {
        let manager = TerminalManager::new();

        let term_id = manager
            .create_terminal(
                "cmd".to_string(),
                vec!["/C".to_string(), "echo done".to_string()],
                None,
                vec![],
                None,
            )
            .await
            .unwrap();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            manager.wait_for_exit(&term_id),
        )
        .await;

        let status = result.expect("wait_for_exit timed out").unwrap();
        match status {
            TerminalStatus::Completed { exit_code, .. } => {
                assert_eq!(exit_code, Some(0));
            }
            other => panic!("Expected Completed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_terminal_not_found() {
        let manager = TerminalManager::new();

        let output = manager.get_output("nonexistent").await;
        assert!(output.is_none());

        let status = manager.get_status("nonexistent").await;
        assert_eq!(status, None);

        let result = manager.kill("nonexistent").await;
        assert!(matches!(result, Err(TerminalError::NotFound(_))));

        let result = manager.release("nonexistent").await;
        assert!(matches!(result, Err(TerminalError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_terminal_env_vars() {
        let manager = TerminalManager::new();

        let term_id = manager
            .create_terminal(
                "cmd".to_string(),
                vec!["/C".to_string(), "echo %MY_VAR%".to_string()],
                None,
                vec![("MY_VAR".to_string(), "test_value".to_string())],
                None,
            )
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let result = manager.get_output(&term_id).await;
        assert!(result.is_some());
        let (output, _, _) = result.unwrap();
        assert!(
            output.contains("test_value"),
            "Expected 'test_value' in output, got: {}",
            output
        );
    }

    #[tokio::test]
    async fn test_multiple_terminals() {
        let manager = TerminalManager::new();

        let term1 = manager
            .create_terminal(
                "cmd".to_string(),
                vec!["/C".to_string(), "echo one".to_string()],
                None,
                vec![],
                None,
            )
            .await
            .unwrap();
        let term2 = manager
            .create_terminal(
                "cmd".to_string(),
                vec!["/C".to_string(), "echo two".to_string()],
                None,
                vec![],
                None,
            )
            .await
            .unwrap();

        assert_ne!(term1, term2);

        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let (output1, _, _) = manager.get_output(&term1).await.unwrap();
        let (output2, _, _) = manager.get_output(&term2).await.unwrap();

        assert!(output1.contains("one"));
        assert!(output2.contains("two"));
    }

    #[tokio::test]
    async fn test_terminal_output_byte_limit() {
        let manager = TerminalManager::new();

        let term_id = manager
            .create_terminal(
                "cmd".to_string(),
                vec!["/C".to_string(), "echo abcdefghijklmnopqrstuvwxyz".to_string()],
                None,
                vec![],
                Some(10),
            )
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let result = manager.get_output(&term_id).await;
        assert!(result.is_some());
        let (_, truncated, _) = result.unwrap();
        assert!(truncated, "Expected output to be truncated");
    }
}
