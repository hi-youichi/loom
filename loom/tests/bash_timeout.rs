//! Integration tests for BashTool timeout detach behavior.
//!
//! These tests use Unix-specific features (sleep, $$, libc::kill) and
//! only run on Unix platforms.

#![cfg(unix)]

mod init_logging;

use loom::tools::{LocalCommandExecutor, Tool};
use loom::tools::bash::CommandExecutor;
use tempfile::TempDir;

#[tokio::test]
async fn normal_command_completes_within_timeout() {
    let executor = LocalCommandExecutor;
    let tmpdir = TempDir::new().unwrap();
    let workdir = tmpdir.path().to_path_buf();

    let result = executor
        .execute(
            "echo hello",
            Some(&workdir),
            Some(30_000),
            vec![],
            None,
        )
        .await
        .unwrap();

    let text = result.as_text().unwrap();
    assert!(text.contains("hello"), "expected 'hello' in output, got: {}", text);

    let shell_dir = workdir.join(".loom").join("shell");
    if shell_dir.exists() {
        let entries: Vec<_> = std::fs::read_dir(shell_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        for date_dir in &entries {
            let files: Vec<_> = std::fs::read_dir(date_dir.path())
                .unwrap()
                .filter_map(|e| e.ok())
                .collect();
            assert!(files.is_empty(), "leftover files after normal completion: {:?}", files);
        }
    }
}

#[tokio::test]
async fn timed_out_command_returns_pid_and_file_paths() {
    let executor = LocalCommandExecutor;
    let tmpdir = TempDir::new().unwrap();
    let workdir = tmpdir.path().to_path_buf();

    let result = executor
        .execute(
            "sleep 30 && echo done",
            Some(&workdir),
            Some(1_000),
            vec![],
            None,
        )
        .await
        .unwrap();

    let text = result.as_text().unwrap();
    assert!(text.contains("Command timed out"), "expected timeout message, got: {}", text);
    assert!(text.contains("PID:"), "expected PID in timeout message, got: {}", text);
    assert!(text.contains(".loom/shell/"), "expected file path in timeout message, got: {}", text);
    assert!(text.contains("Use `kill"), "expected kill hint, got: {}", text);
    assert!(text.contains("Use `cat"), "expected cat hint, got: {}", text);
}

#[tokio::test]
async fn timed_out_command_output_files_exist() {
    let executor = LocalCommandExecutor;
    let tmpdir = TempDir::new().unwrap();
    let workdir = tmpdir.path().to_path_buf();

    let result = executor
        .execute(
            "echo before_timeout && sleep 30",
            Some(&workdir),
            Some(1_500),
            vec![],
            None,
        )
        .await
        .unwrap();

    let text = result.as_text().unwrap();
    assert!(text.contains("before_timeout"), "expected partial output, got: {}", text);

    let stdout_file = text
        .lines()
        .find(|l: &&str| l.starts_with("stdout: "))
        .expect("expected stdout file path")
        .strip_prefix("stdout: ")
        .unwrap()
        .trim();
    let stdout_path = workdir.join(stdout_file);
    assert!(stdout_path.exists(), "stdout file should exist at {:?}", stdout_path);

    let file_content = std::fs::read_to_string(&stdout_path).unwrap();
    assert!(file_content.contains("before_timeout"), "file content should contain partial output");
}

#[tokio::test]
async fn timed_out_command_stderr_file_exists_if_stderr() {
    let executor = LocalCommandExecutor;
    let tmpdir = TempDir::new().unwrap();
    let workdir = tmpdir.path().to_path_buf();

    let result: Result<loom::tool_source::ToolCallContent, loom::tool_source::ToolSourceError> = executor
        .execute(
            "echo stdout_msg && echo stderr_msg >&2 && sleep 30",
            Some(&workdir),
            Some(1_500),
            vec![],
            None,
        )
        .await;

    let content = result.unwrap();
    let text = content.as_text().unwrap();
    assert!(text.contains("Partial Stderr"), "expected stderr section, got: {}", text);
    assert!(text.contains("stderr_msg"), "expected stderr content, got: {}", text);
}

#[tokio::test]
async fn timed_out_process_is_still_running() {
    let executor = LocalCommandExecutor;
    let tmpdir = TempDir::new().unwrap();
    let workdir = tmpdir.path().to_path_buf();

    let pid_file = workdir.join("child_pid.txt");
    let result = executor
        .execute(
            &format!("echo $$ > {} && sleep 30", pid_file.display()),
            Some(&workdir),
            Some(1_000),
            vec![],
            None,
        )
        .await
        .unwrap();

    let text = result.as_text().unwrap();
    assert!(text.contains("Command timed out"), "expected timeout, got: {}", text);

    let reported_pid: u32 = text
        .lines()
        .find(|l: &&str| l.contains("PID:"))
        .and_then(|l| {
            let start = l.find("PID: ")?;
            let rest = &l[start + 5..];
            rest.split(|c: char| !c.is_ascii_digit()).next()
        })
        .and_then(|s| s.trim().parse().ok())
        .expect("expected PID in output");

    let kill_result = unsafe { libc::kill(reported_pid as i32, 0) };
    assert_eq!(kill_result, 0, "process with PID {} should still be running", reported_pid);

    unsafe { libc::kill(reported_pid as i32, libc::SIGKILL); }
}

#[tokio::test]
async fn default_timeout_used_when_zero() {
    let executor = LocalCommandExecutor;
    let tmpdir = TempDir::new().unwrap();
    let workdir = tmpdir.path().to_path_buf();

    let result = executor
        .execute(
            "echo default_timeout",
            Some(&workdir),
            Some(0),
            vec![],
            None,
        )
        .await
        .unwrap();

    let text = result.as_text().unwrap();
    assert!(text.contains("default_timeout"), "expected output, got: {}", text);
}

#[tokio::test]
async fn default_timeout_used_when_none() {
    let executor = LocalCommandExecutor;
    let tmpdir = TempDir::new().unwrap();
    let workdir = tmpdir.path().to_path_buf();

    let result: Result<loom::tool_source::ToolCallContent, loom::tool_source::ToolSourceError> = executor
        .execute(
            "echo none_timeout",
            Some(&workdir),
            None,
            vec![],
            None,
        )
        .await;

    let content = result.unwrap();
    let text = content.as_text().unwrap();
    assert!(text.contains("none_timeout"), "expected output, got: {}", text);
}

#[tokio::test]
async fn shell_output_dir_uses_working_dir() {
    let executor = LocalCommandExecutor;
    let tmpdir = TempDir::new().unwrap();
    let workdir = tmpdir.path().to_path_buf();

    let result = executor
        .execute(
            "echo in_workdir && sleep 30",
            Some(&workdir),
            Some(1_000),
            vec![],
            None,
        )
        .await
        .unwrap();

    let text = result.as_text().unwrap();
    assert!(text.contains(".loom/shell/"), "expected relative path, got: {}", text);
    assert!(!text.contains("/private/"), "should use relative paths, got: {}", text);
}

#[tokio::test]
async fn normal_command_cleans_up_output_files() {
    let executor = LocalCommandExecutor;
    let tmpdir = TempDir::new().unwrap();
    let workdir = tmpdir.path().to_path_buf();

    let _ = executor
        .execute(
            "echo cleanup_test",
            Some(&workdir),
            Some(30_000),
            vec![],
            None,
        )
        .await
        .unwrap();

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let shell_dir = workdir.join(".loom").join("shell").join(&today);
    if shell_dir.exists() {
        let files: Vec<_> = std::fs::read_dir(&shell_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(files.is_empty(), "expected no leftover files, found: {:?}", files);
    }
}

#[tokio::test]
async fn timed_out_command_relative_paths() {
    let executor = LocalCommandExecutor;
    let tmpdir = TempDir::new().unwrap();
    let workdir = tmpdir.path().to_path_buf();

    let result = executor
        .execute(
            "echo relative && sleep 30",
            Some(&workdir),
            Some(1_000),
            vec![],
            None,
        )
        .await
        .unwrap();

    let text = result.as_text().unwrap();

    let stdout_line = text
        .lines()
        .find(|l: &&str| l.starts_with("stdout: "))
        .expect("expected stdout file line");

    let path_str = stdout_line.strip_prefix("stdout: ").unwrap().trim();
    assert!(path_str.starts_with(".loom/shell/"), "expected relative path, got: {}", path_str);
    assert!(path_str.ends_with(".stdout"), "expected .stdout suffix, got: {}", path_str);
}

#[tokio::test]
async fn normal_command_with_stderr() {
    let executor = LocalCommandExecutor;
    let tmpdir = TempDir::new().unwrap();
    let workdir = tmpdir.path().to_path_buf();

    let result: Result<loom::tool_source::ToolCallContent, loom::tool_source::ToolSourceError> = executor
        .execute(
            "echo stdout_msg && echo stderr_msg >&2",
            Some(&workdir),
            Some(30_000),
            vec![],
            None,
        )
        .await;

    let content = result.unwrap();
    let text = content.as_text().unwrap();
    assert!(text.contains("stderr:"), "expected stderr section, got: {}", text);
    assert!(text.contains("stderr_msg"), "expected stderr content, got: {}", text);
}

#[tokio::test]
async fn bash_tool_spec_mentions_background_timeout() {
    let tool = loom::tools::BashTool::new();
    let spec = tool.spec();
    let desc = spec.description.unwrap();
    assert!(
        desc.contains("background") || desc.contains("PID") || desc.contains("timeout"),
        "spec should mention timeout/background behavior, got: {}",
        desc
    );
}

#[tokio::test]
async fn bash_tool_yaml_has_timeout_description() {
    let yaml_str = include_str!("../tools/bash.yaml");
    assert!(
        yaml_str.contains("background") || yaml_str.contains("timeout"),
        "bash.yaml should mention background/timeout behavior"
    );
}
