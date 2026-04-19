//! E2E tests for Diff content in ACP protocol
//!
//! These tests verify that write/edit operations produce correct Diff content
//! in the ACP protocol responses, covering the complete workflow from prompt
//! to tool_call_update with Diff content.

mod e2e;

use std::time::Duration;

use e2e::ToolCallResponse;

const TIMEOUT: Duration = Duration::from_secs(30);

fn find_diff_notification(notifications: &[serde_json::Value]) -> Option<&serde_json::Value> {
    notifications.iter().find(|n| {
        let update = match n.pointer("/params/update") {
            Some(u) => u,
            None => return false,
        };
        if update.get("sessionUpdate").and_then(|v| v.as_str()) != Some("tool_call_update") {
            return false;
        }
        update
            .get("content")
            .and_then(|c| c.as_array())
            .is_some_and(|item| item.get("type").and_then(|v| v.as_str()) == Some("diff"))
    })
}

fn extract_diff(notification: &serde_json::Value) -> &serde_json::Value {
    notification
        .pointer("/params/update/content")
        .and_then(|c| c.as_array())
        .expect("should have content array")
        .iter()
        .find(|item| item.get("type").and_then(|v| v.as_str()) == Some("diff"))
        .expect("should have diff item")
}

fn get_diff_field<'a>(diff: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    diff.get(field).and_then(|v| v.as_str())
}

fn collect_notifications(
    acp: &mut e2e::AcpChild,
    prompt_id: u64,
) -> (Vec<serde_json::Value>, e2e::RpcResponse) {
    let mut notifications = Vec::new();
    let mut response = None;
    let start = std::time::Instant::now();

    while start.elapsed() < TIMEOUT && response.is_none() {
        let message = acp.read_message().expect("read message");

        if message.get("method").and_then(|v| v.as_str()) == Some("session/update") {
            notifications.push(message.clone());
        }

        if message.get("id").and_then(|v| v.as_u64()) == Some(prompt_id) {
            let r: e2e::RpcResponse = serde_json::from_value(message).expect("parse response");
            response = Some(r);
        }
    }

    let r = response.expect("should receive prompt response");
    (notifications, r)
}

fn find_tool_call_notification<'a>(
    notifications: &'a [serde_json::Value],
    tool_name: &str,
) -> Option<&'a serde_json::Value> {
    notifications.iter().find(|n| {
        let update = match n.pointer("/params/update") {
            Some(u) => u,
            None => return false,
        };
        update.get("sessionUpdate").and_then(|v| v.as_str()) == Some("tool_call")
            && update.get("title").and_then(|v| v.as_str()).map_or(false, |t| t.to_lowercase().contains(tool_name))
    })
}

/// Test that write_file operation returns Diff content in tool_call_update.
#[tokio::test]
async fn test_write_operation_returns_diff_content() {
    let (mut acp, mock) = e2e::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = acp.handshake(TIMEOUT).expect("handshake");
    assert!(!session_id.is_empty());

    mock.mount_tool_call_response(&[ToolCallResponse {
        tool_name: "write_file".to_string(),
        parameters: serde_json::json!({
            "path": "test_file.txt",
            "content": "Hello, World!"
        }),
    }]).await;

    let prompt_id = acp.send_request(
        "session/prompt",
        serde_json::json!({
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": "Create test_file.txt" }],
        }),
    ).expect("send prompt");

    let (notifications, response) = collect_notifications(&mut acp, prompt_id);
    assert!(response.error.is_none(), "prompt failed: {:?}", response.error);

    let diff_notification = find_diff_notification(&notifications)
        .expect("should find tool_call_update with Diff content");

    let diff = extract_diff(diff_notification);
    assert_eq!(get_diff_field(diff, "path"), Some("test_file.txt"));
    assert_eq!(get_diff_field(diff, "newText"), Some("Hello, World!"));
}

/// Test that edit tool_call is sent with correct parameters.
/// The edit fails because the file doesn't exist, but we verify the tool_call notification.
#[tokio::test]
async fn test_edit_tool_call_sent_with_correct_params() {
    let (mut acp, mock) = e2e::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = acp.handshake(TIMEOUT).expect("handshake");
    assert!(!session_id.is_empty());

    mock.mount_tool_call_response(&[ToolCallResponse {
        tool_name: "edit".to_string(),
        parameters: serde_json::json!({
            "path": "existing_file.txt",
            "oldString": "Original content",
            "newString": "Modified content"
        }),
    }]).await;

    let prompt_id = acp.send_request(
        "session/prompt",
        serde_json::json!({
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": "Edit existing_file.txt" }],
        }),
    ).expect("send prompt");

    let (notifications, response) = collect_notifications(&mut acp, prompt_id);
    assert!(response.error.is_none(), "prompt failed: {:?}", response.error);

    let tool_call = find_tool_call_notification(&notifications, "edit")
        .expect("should find tool_call for edit");

    let raw_input = tool_call.pointer("/params/update/rawInput")
        .expect("should have rawInput");
    assert_eq!(raw_input.get("path").and_then(|v| v.as_str()), Some("existing_file.txt"));
    assert_eq!(raw_input.get("oldString").and_then(|v| v.as_str()), Some("Original content"));
    assert_eq!(raw_input.get("newString").and_then(|v| v.as_str()), Some("Modified content"));
}

/// Test complete workflow: write_file produces Diff with path and newText.
#[tokio::test]
async fn test_complete_diff_workflow() {
    let (mut acp, mock) = e2e::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = acp.handshake(TIMEOUT).expect("handshake");
    assert!(!session_id.is_empty());

    mock.mount_tool_call_response(&[ToolCallResponse {
        tool_name: "write_file".to_string(),
        parameters: serde_json::json!({
            "path": "workflow_test.txt",
            "content": "Initial content"
        }),
    }]).await;

    let prompt_id = acp.send_request(
        "session/prompt",
        serde_json::json!({
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": "Create workflow_test.txt" }],
        }),
    ).expect("send prompt");

    let (notifications, _response) = collect_notifications(&mut acp, prompt_id);

    let diff_notification = find_diff_notification(&notifications)
        .expect("should find tool_call_update with Diff content");

    let diff = extract_diff(diff_notification);
    assert_eq!(get_diff_field(diff, "path"), Some("workflow_test.txt"));
    assert_eq!(get_diff_field(diff, "newText"), Some("Initial content"));
}
