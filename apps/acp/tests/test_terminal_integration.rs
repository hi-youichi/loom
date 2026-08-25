#[cfg(test)]
mod tests {
    use anureo_acp::terminal::{TerminalError, TerminalManager, TerminalStatus};

    fn echo_cmd(msg: &str) -> (String, Vec<String>) {
        if cfg!(windows) {
            (
                "powershell".to_string(),
                vec![
                    "-NoProfile".to_string(),
                    "-Command".to_string(),
                    format!("echo {}", msg),
                ],
            )
        } else {
            (
                "/bin/sh".to_string(),
                vec!["-c".to_string(), format!("echo {}", msg)],
            )
        }
    }

    fn long_running_cmd() -> (String, Vec<String>) {
        if cfg!(windows) {
            (
                "powershell".to_string(),
                vec![
                    "-NoProfile".to_string(),
                    "-Command".to_string(),
                    "Start-Sleep -Seconds 60".to_string(),
                ],
            )
        } else {
            (
                "/bin/sh".to_string(),
                vec!["-c".to_string(), "sleep 60".to_string()],
            )
        }
    }

    fn env_echo_cmd() -> (String, Vec<String>) {
        if cfg!(windows) {
            (
                "powershell".to_string(),
                vec![
                    "-NoProfile".to_string(),
                    "-Command".to_string(),
                    "Write-Output $env:MY_VAR".to_string(),
                ],
            )
        } else {
            (
                "/bin/sh".to_string(),
                vec!["-c".to_string(), "echo $MY_VAR".to_string()],
            )
        }
    }

    #[tokio::test]
    async fn test_terminal_create_and_output() {
        let manager = TerminalManager::new();
        let (cmd, args) = echo_cmd("hello");

        let term_id = manager
            .create_terminal(cmd, args, None, vec![], None)
            .await
            .unwrap();

        assert!(term_id.starts_with("term-"));

        let (_output, _truncated, status) =
            tokio::time::timeout(std::time::Duration::from_secs(3), async {
                loop {
                    if let Some((output, truncated, status)) = manager.get_output(&term_id).await {
                        if output.contains("hello") {
                            return (output, truncated, status);
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            })
            .await
            .expect("timed out waiting for output");

        if let Some(TerminalStatus::Completed { exit_code, .. }) = status {
            assert_eq!(exit_code, Some(0));
        }
    }

    #[tokio::test]
    async fn test_terminal_kill() {
        let manager = TerminalManager::new();
        let (cmd, args) = long_running_cmd();

        let term_id = manager
            .create_terminal(cmd, args, None, vec![], None)
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let _status_before = manager.get_status(&term_id).await;

        manager.kill(&term_id).await.unwrap();

        let _status = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if let Some(TerminalStatus::Killed) = manager.get_status(&term_id).await {
                    return TerminalStatus::Killed;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("timed out waiting for kill");
    }

    #[tokio::test]
    async fn test_terminal_release() {
        let manager = TerminalManager::new();
        let (cmd, args) = long_running_cmd();

        let term_id = manager
            .create_terminal(cmd, args, None, vec![], None)
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        manager.release(&term_id).await.unwrap();

        let status = manager.get_status(&term_id).await;
        assert_eq!(status, Some(TerminalStatus::Released));

        let result = manager.kill(&term_id).await;
        assert!(matches!(result, Err(TerminalError::AlreadyReleased(_))));
    }

    #[tokio::test]
    async fn test_terminal_wait_for_exit() {
        let manager = TerminalManager::new();
        let (cmd, args) = echo_cmd("done");

        let term_id = manager
            .create_terminal(cmd, args, None, vec![], None)
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
        let (cmd, args) = env_echo_cmd();

        let term_id = manager
            .create_terminal(
                cmd,
                args,
                None,
                vec![("MY_VAR".to_string(), "test_value".to_string())],
                None,
            )
            .await
            .unwrap();

        let (_output, _, _) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if let Some((output, truncated, status)) = manager.get_output(&term_id).await {
                    if output.contains("test_value") {
                        return (output, truncated, status);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("timed out waiting for output");
    }

    #[tokio::test]
    async fn test_multiple_terminals() {
        let manager = TerminalManager::new();

        let (cmd1, args1) = echo_cmd("one");
        let (cmd2, args2) = echo_cmd("two");

        let term1 = manager
            .create_terminal(cmd1, args1, None, vec![], None)
            .await
            .unwrap();
        let term2 = manager
            .create_terminal(cmd2, args2, None, vec![], None)
            .await
            .unwrap();

        assert_ne!(term1, term2);

        let (_output1, _output2) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let r1 = manager.get_output(&term1).await;
                let r2 = manager.get_output(&term2).await;
                if let (Some((o1, _, _)), Some((o2, _, _))) = (r1, r2) {
                    if o1.contains("one") && o2.contains("two") {
                        return (o1, o2);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("timed out waiting for output");
    }

    #[tokio::test]
    async fn test_terminal_output_byte_limit() {
        let manager = TerminalManager::new();
        let (cmd, args) = echo_cmd("abcdefghijklmnopqrstuvwxyz");

        let term_id = manager
            .create_terminal(cmd, args, None, vec![], Some(10))
            .await
            .unwrap();

        let (_, _truncated, _) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if let Some((output, truncated, status)) = manager.get_output(&term_id).await {
                    if truncated {
                        return (output, truncated, status);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("timed out waiting for output");
    }
}
