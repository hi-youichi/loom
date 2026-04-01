#[cfg(test)]
mod tests {
    use loom_acp::terminal::{TerminalManager, TerminalStatus};

    #[tokio::test]
    async fn test_terminal_lifecycle() {
        let manager = TerminalManager::new();

        let term_id = manager
            .create_terminal(
                "echo".to_string(),
                Some(vec!["Hello".to_string()]),
                Some("/tmp".to_string()),
            )
            .await
            .unwrap();

        assert!(term_id.starts_with("term-"));

        let status = manager.get_status(&term_id).await;
        assert_eq!(status, Some(TerminalStatus::Running));

        manager.append_output(&term_id, "Hello\n").await;

        let output = manager.get_output(&term_id).await;
        assert_eq!(output, Some("Hello\n".to_string()));

        manager
            .update_status(&term_id, TerminalStatus::Completed { exit_code: 0 })
            .await;

        let status = manager.get_status(&term_id).await;
        assert_eq!(status, Some(TerminalStatus::Completed { exit_code: 0 }));
    }

    #[tokio::test]
    async fn test_terminal_output_streaming() {
        let manager = TerminalManager::new();

        let term_id = manager
            .create_terminal("ls".to_string(), None, None)
            .await
            .unwrap();

        for i in 0..5 {
            manager
                .append_output(&term_id, &format!("Line {}\n", i))
                .await;
        }

        let output = manager.get_output(&term_id).await.unwrap();
        assert!(output.contains("Line 0"));
        assert!(output.contains("Line 4"));
    }

    #[tokio::test]
    async fn test_multiple_terminals() {
        let manager = TerminalManager::new();

        let term1 = manager
            .create_terminal("cmd1".to_string(), None, None)
            .await
            .unwrap();
        let term2 = manager
            .create_terminal("cmd2".to_string(), None, None)
            .await
            .unwrap();

        assert_ne!(term1, term2);

        manager.append_output(&term1, "output1\n").await;
        manager.append_output(&term2, "output2\n").await;

        let output1 = manager.get_output(&term1).await.unwrap();
        let output2 = manager.get_output(&term2).await.unwrap();

        assert_eq!(output1, "output1\n");
        assert_eq!(output2, "output2\n");
    }

    #[tokio::test]
    async fn test_terminal_status_transitions() {
        let manager = TerminalManager::new();

        let term_id = manager
            .create_terminal("test".to_string(), None, None)
            .await
            .unwrap();

        assert_eq!(
            manager.get_status(&term_id).await,
            Some(TerminalStatus::Running)
        );

        manager
            .update_status(&term_id, TerminalStatus::Completed { exit_code: 0 })
            .await;
        assert_eq!(
            manager.get_status(&term_id).await,
            Some(TerminalStatus::Completed { exit_code: 0 })
        );

        let term_id2 = manager
            .create_terminal("fail".to_string(), None, None)
            .await
            .unwrap();
        manager
            .update_status(
                &term_id2,
                TerminalStatus::Failed {
                    error: "Error".to_string(),
                },
            )
            .await;
        assert_eq!(
            manager.get_status(&term_id2).await,
            Some(TerminalStatus::Failed {
                error: "Error".to_string()
            })
        );
    }

    #[tokio::test]
    async fn test_terminal_not_found() {
        let manager = TerminalManager::new();

        let output = manager.get_output("nonexistent").await;
        assert_eq!(output, None);

        let status = manager.get_status("nonexistent").await;
        assert_eq!(status, None);
    }
}
