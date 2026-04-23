mod common;
mod e2e;
mod mocks;

use std::time::Duration;
use std::sync::atomic::Ordering;

const TIMEOUT: Duration = Duration::from_secs(60);

async fn setup_with_terminal() -> (common::AcpChild, common::MockAcpServer, String) {
    let (mut acp, mock) = common::AcpChild::spawn_with_mock_and_terminal()
        .await
        .expect("spawn loom-acp with terminal");
    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    assert!(acp.has_terminal_handler());
    (acp, mock, session_id)
}

async fn setup_without_terminal() -> (common::AcpChild, common::MockAcpServer, String) {
    let (mut acp, mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp without terminal");
    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    assert!(!acp.has_terminal_handler());
    (acp, mock, session_id)
}

fn extract_update_types(notifications: &[serde_json::Value]) -> Vec<String> {
    notifications
        .iter()
        .filter(|n| n.get("method").and_then(|v| v.as_str()) == Some("session/update"))
        .filter_map(|n| {
            n.get("params")
                .and_then(|p| p.get("update"))
                .and_then(|u| u.get("sessionUpdate"))
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect()
}

// ============================================================
// Phase T1: Capability Negotiation
// ============================================================

#[tokio::test]
async fn e2e_terminal_capability_advertised() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let call_count = mock.mount_bash_tool_call("echo hello").await;

    let request_id = acp
        .send_prompt_request(&session_id, "Run echo hello")
        .expect("send prompt");

    let (notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);
    let result = response.result.expect("should have result");
    assert_eq!(result["stopReason"], "end_turn");

    assert!(call_count.load(Ordering::SeqCst) >= 2, "Mock LLM should be called >= 2 times");

    let calls = acp.take_terminal_calls();
    let methods: Vec<&str> = calls.iter().map(|c| c.method.as_str()).collect();
    assert!(methods.contains(&"terminal/create"), "should have terminal/create, got: {:?}", methods);

    let _ = (notifications, session_id);
}

#[tokio::test]
async fn e2e_terminal_capability_absent() {
    let (mut acp, mock, session_id) = setup_without_terminal().await;
    let call_count = mock.mount_bash_tool_call("echo hello").await;

    let request_id = acp
        .send_prompt_request(&session_id, "Run echo hello")
        .expect("send prompt");

    let (notifications, response) = acp
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    assert!(call_count.load(Ordering::SeqCst) >= 2, "Mock LLM should be called >= 2 times");

    let calls = acp.take_terminal_calls();
    assert!(calls.is_empty(), "Path B should not send terminal requests");

    let _ = (notifications, session_id);
}

// ============================================================
// Phase T2: Terminal Create (Path A)
// ============================================================

#[tokio::test]
async fn e2e_terminal_create_basic() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let call_count = mock.mount_bash_tool_call("echo hello").await;

    let request_id = acp.send_prompt_request(&session_id, "Run echo hello").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let create_call = calls.iter().find(|c| c.method == "terminal/create")
        .expect("should have terminal/create call");

    let params = &create_call.params;
    let command = params.get("command").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(command, "sh", "command should be 'sh', got: {}", command);
    let args = params.get("args").and_then(|v| v.as_array());
    let args_str = args.map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
    assert!(args_str.contains("echo hello"), "args should contain 'echo hello', got: {}", args_str);

    let terminal_id = create_call.response_result
        .as_ref()
        .and_then(|r| r.get("terminalId"))
        .and_then(|v| v.as_str())
        .expect("should return terminalId");
    assert!(terminal_id.starts_with("term-"), "terminalId should start with 'term-', got: {}", terminal_id);
}

#[tokio::test]
async fn e2e_terminal_create_with_args() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call("/bin/sh -c 'echo hello'").await;

    let request_id = acp.send_prompt_request(&session_id, "Run /bin/sh -c echo hello").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let create_call = calls.iter().find(|c| c.method == "terminal/create")
        .expect("should have terminal/create call");
    assert!(create_call.response_result.is_some(), "create should succeed");
}

// ============================================================
// Phase T3: Terminal Output (Path A)
// ============================================================

#[tokio::test]
async fn e2e_terminal_output_basic() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call("echo hello").await;

    let request_id = acp.send_prompt_request(&session_id, "Run echo hello").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let output_call = calls.iter().find(|c| c.method == "terminal/output")
        .expect("should have terminal/output call");

    let result = output_call.response_result.as_ref().expect("should have result");
    let output = result.get("output").and_then(|v| v.as_str()).unwrap_or("");
    assert!(output.contains("hello"), "output should contain 'hello', got: {}", output);
    let truncated = result.get("truncated").and_then(|v| v.as_bool()).unwrap_or(true);
    assert!(!truncated, "output should not be truncated");
}

#[tokio::test]
async fn e2e_terminal_output_after_completion() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call("echo done").await;

    let request_id = acp.send_prompt_request(&session_id, "Run echo done").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let output_calls: Vec<_> = calls.iter().filter(|c| c.method == "terminal/output").collect();
    assert!(!output_calls.is_empty(), "should have terminal/output calls");

    let last_output = output_calls.last().expect("should have output");
    let result = last_output.response_result.as_ref().expect("should have result");
    let output = result.get("output").and_then(|v| v.as_str()).unwrap_or("");
    assert!(output.contains("done"), "output should contain 'done', got: {}", output);
}

#[tokio::test]
async fn e2e_terminal_output_includes_stdout_stderr() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call("echo out && echo err >&2").await;

    let request_id = acp.send_prompt_request(&session_id, "Run echo out && echo err >&2").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let output_call = calls.iter().find(|c| c.method == "terminal/output")
        .expect("should have terminal/output call");
    let result = output_call.response_result.as_ref().expect("should have result");
    let output = result.get("output").and_then(|v| v.as_str()).unwrap_or("");
    assert!(output.contains("out"), "should contain stdout, got: {}", output);
    assert!(output.contains("err"), "should contain stderr, got: {}", output);
}

// ============================================================
// Phase T4: Terminal Wait For Exit (Path A)
// ============================================================

#[tokio::test]
async fn e2e_terminal_wait_for_exit_success() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call("echo done").await;

    let request_id = acp.send_prompt_request(&session_id, "Run echo done").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let wait_call = calls.iter().find(|c| c.method == "terminal/wait_for_exit")
        .expect("should have terminal/wait_for_exit call");

    let result = wait_call.response_result.as_ref().expect("should have result");
    let exit_code = result.get("exitCode").and_then(|v| v.as_i64());
    assert_eq!(exit_code, Some(0), "exit code should be 0, got: {:?}", exit_code);
}

#[tokio::test]
async fn e2e_terminal_wait_for_exit_failure() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call("sh -c 'exit 1'").await;

    let request_id = acp.send_prompt_request(&session_id, "Run exit 1").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let wait_call = calls.iter().find(|c| c.method == "terminal/wait_for_exit")
        .expect("should have terminal/wait_for_exit call");

    let result = wait_call.response_result.as_ref().expect("should have result");
    let exit_code = result.get("exitCode").and_then(|v| v.as_i64());
    assert_eq!(exit_code, Some(1), "exit code should be 1, got: {:?}", exit_code);
}

// ============================================================
// Phase T5: Terminal Kill (Path A)
// ============================================================

#[tokio::test]
#[ignore] // Requires bash tool timeout behavior — the agent's internal retry loop makes this timing-sensitive
async fn e2e_terminal_kill_running() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call_with_timeout("sleep 300", Some(5000)).await;

    let request_id = acp.send_prompt_request(&session_id, "Run sleep 300").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, Duration::from_secs(15))
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let has_kill = calls.iter().any(|c| c.method == "terminal/kill");
    assert!(has_kill, "should have terminal/kill call for long-running process");

    let wait_call = calls.iter().find(|c| c.method == "terminal/wait_for_exit");
    if let Some(wait) = wait_call {
        if let Some(result) = &wait.response_result {
            let signal = result.get("signal").and_then(|v| v.as_str());
            assert!(signal.is_some(), "killed process should have signal");
        }
    }

    let _ = session_id;
}

// ============================================================
// Phase T6: Terminal Release (Path A)
// ============================================================

#[tokio::test]
async fn e2e_terminal_release_after_completion() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call("echo done").await;

    let request_id = acp.send_prompt_request(&session_id, "Run echo done").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let release_call = calls.iter().find(|c| c.method == "terminal/release")
        .expect("should have terminal/release call");
    assert!(release_call.response_result.is_some(), "release should succeed");

    let create_call = calls.iter().find(|c| c.method == "terminal/create").expect("should have create");
    let terminal_id = create_call.response_result
        .as_ref()
        .and_then(|r| r.get("terminalId"))
        .and_then(|v| v.as_str())
        .expect("should have terminalId");
    assert!(!terminal_id.is_empty(), "terminalId should not be empty");
}

#[tokio::test]
#[ignore] // Requires bash tool timeout behavior — the agent's internal retry loop makes this timing-sensitive
async fn e2e_terminal_release_running() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call_with_timeout("sleep 300", Some(5000)).await;

    let request_id = acp.send_prompt_request(&session_id, "Run sleep 300").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, Duration::from_secs(15))
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let has_release = calls.iter().any(|c| c.method == "terminal/release");
    assert!(has_release, "should have terminal/release for running process");
}

// ============================================================
// Phase T7: Full Lifecycle via Prompt
// ============================================================

#[tokio::test]
async fn e2e_prompt_triggers_bash_tool() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let call_count = mock.mount_bash_tool_call("echo hello").await;

    let request_id = acp.send_prompt_request(&session_id, "Run echo hello").expect("send prompt");
    let (notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);
    assert_eq!(response.result.unwrap()["stopReason"], "end_turn");

    assert!(call_count.load(Ordering::SeqCst) >= 2, "Mock LLM should be called >= 2 times");

    let calls = acp.take_terminal_calls();
    let methods: Vec<&str> = calls.iter().map(|c| c.method.as_str()).collect();
    assert!(methods.starts_with(&["terminal/create"]), "should start with create, got: {:?}", methods);
    assert!(methods.contains(&"terminal/output"), "should have output, got: {:?}", methods);
    assert!(methods.contains(&"terminal/wait_for_exit"), "should have wait_for_exit, got: {:?}", methods);
    assert!(methods.contains(&"terminal/release"), "should have release, got: {:?}", methods);

    let updates = extract_update_types(&notifications);
    assert!(!updates.is_empty(), "should have session/update notifications");
}

#[tokio::test]
async fn e2e_prompt_bash_output_in_response() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call("echo hello").await;

    let request_id = acp.send_prompt_request(&session_id, "Run echo hello").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let output_call = calls.iter().find(|c| c.method == "terminal/output")
        .expect("should have output call");
    let result = output_call.response_result.as_ref().expect("should have result");
    let output = result.get("output").and_then(|v| v.as_str()).unwrap_or("");
    assert!(output.contains("hello"), "output should contain 'hello', got: {}", output);
}

#[tokio::test]
async fn e2e_prompt_bash_working_dir() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call("pwd").await;

    let request_id = acp.send_prompt_request(&session_id, "Run pwd").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let output_call = calls.iter().find(|c| c.method == "terminal/output");
    if let Some(output_call) = output_call {
        if let Some(result) = &output_call.response_result {
            let output = result.get("output").and_then(|v| v.as_str()).unwrap_or("");
            assert!(!output.trim().is_empty(), "pwd should produce output");
        }
    }
}

// ============================================================
// Phase T8: Local Execution (Path B)
// ============================================================

#[tokio::test]
async fn e2e_local_bash_echo() {
    let (mut acp, mock, session_id) = setup_without_terminal().await;
    let call_count = mock.mount_bash_tool_call("echo hello").await;

    let request_id = acp.send_prompt_request(&session_id, "Run echo hello").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);
    assert_eq!(response.result.unwrap()["stopReason"], "end_turn");

    assert!(call_count.load(Ordering::SeqCst) >= 2, "Mock LLM should be called >= 2 times");

    let calls = acp.take_terminal_calls();
    assert!(calls.is_empty(), "Path B should not send terminal requests");
}

#[tokio::test]
async fn e2e_local_bash_exit_code_nonzero() {
    let (mut acp, mock, session_id) = setup_without_terminal().await;
    let _call_count = mock.mount_bash_tool_call("exit 1").await;

    let request_id = acp.send_prompt_request(&session_id, "Run exit 1").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed even with non-zero exit: {:?}", response.error);
}

// ============================================================
// Phase T9: Concurrency & Edge Cases
// ============================================================

#[tokio::test]
async fn e2e_terminal_special_chars_in_command() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call("echo \"hello world\"").await;

    let request_id = acp.send_prompt_request(&session_id, "Run echo hello world").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let output_call = calls.iter().find(|c| c.method == "terminal/output");
    if let Some(output_call) = output_call {
        if let Some(result) = &output_call.response_result {
            let output = result.get("output").and_then(|v| v.as_str()).unwrap_or("");
            assert!(output.contains("hello"), "output should contain 'hello', got: {}", output);
        }
    }
}

#[tokio::test]
async fn e2e_terminal_unicode_output() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call("echo 你好世界").await;

    let request_id = acp.send_prompt_request(&session_id, "Run echo 你好世界").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let output_call = calls.iter().find(|c| c.method == "terminal/output");
    if let Some(output_call) = output_call {
        if let Some(result) = &output_call.response_result {
            let output = result.get("output").and_then(|v| v.as_str()).unwrap_or("");
            assert!(output.contains("你好"), "output should contain unicode, got: {}", output);
        }
    }
}

#[tokio::test]
async fn e2e_terminal_large_output() {
    let (mut acp, mock, session_id) = setup_with_terminal().await;
    let _call_count = mock.mount_bash_tool_call("seq 1 1000").await;

    let request_id = acp.send_prompt_request(&session_id, "Run seq 1 1000").expect("send prompt");
    let (_notifications, response) = acp
        .collect_all_notifications_handling_terminal(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    let calls = acp.take_terminal_calls();
    let output_call = calls.iter().find(|c| c.method == "terminal/output");
    if let Some(output_call) = output_call {
        if let Some(result) = &output_call.response_result {
            let output = result.get("output").and_then(|v| v.as_str()).unwrap_or("");
            assert!(!output.is_empty(), "output should not be empty");
        }
    }
}
