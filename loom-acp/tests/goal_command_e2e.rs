//! E2E test for the /goal command via ACP protocol.
//!
//! Verifies that `/goal <description>` is correctly routed through the ACP prompt
//! handler, triggers the goal runner, and returns a proper `PromptResponse`.

mod common;
mod e2e;

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(120);

/// Send `/goal test objective` and verify the ACP agent returns `end_turn`.
///
/// This tests the full command routing path:
///   session/prompt → command parse → GoalRunner → PromptResponse
///
/// With a mock LLM that just returns "Done." without task_update tool calls,
/// the goal runner will hit max iterations and return an Error outcome,
/// but the ACP layer should always return a valid `PromptResponse`
/// with `stopReason: "end_turn"`.
#[tokio::test]
async fn e2e_goal_command_returns_end_turn() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;
    assert!(!session_id.is_empty(), "session_id should not be empty");

    // Send /goal command via ACP session/prompt
    let request_id = guard
        .acp_mut()
        .send_prompt_request(&session_id, "/goal test the goal feature")
        .expect("send /goal prompt");

    let (notifications, response) = guard
        .acp_mut()
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect /goal response");

    // Should succeed (no error at the ACP protocol level)
    assert!(
        response.error.is_none(),
        "/goal prompt should succeed at protocol level, got error: {:?}",
        response.error
    );

    // Should return end_turn stop reason
    let result = response.result.expect("should have result");
    let stop_reason = result
        .get("stopReason")
        .and_then(|v| v.as_str())
        .expect("should have stopReason");
    assert_eq!(
        stop_reason, "end_turn",
        "expected stopReason 'end_turn', got '{}'",
        stop_reason
    );

    // Log notifications for debugging
    eprintln!(
        "[goal_e2e] received {} notifications",
        notifications.len()
    );
    for notif in &notifications {
        let method = notif
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let update_type = notif
            .get("params")
            .and_then(|p| p.get("update"))
            .and_then(|u| u.get("sessionUpdate"))
            .and_then(|v| v.as_str())
            .unwrap_or("n/a");
        eprintln!("[goal_e2e] notification: method={} update_type={}", method, update_type);
    }
}

/// Send `/goal` without a description and verify it falls through
/// as a normal prompt (not a goal command).
#[tokio::test]
async fn e2e_goal_no_description_is_normal_prompt() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    // /goal without args should not be parsed as a Goal command,
    // it falls through as regular text
    let response = guard
        .acp_mut()
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": [{
                    "type": "text",
                    "text": "/goal"
                }],
            }),
            Duration::from_secs(15),
        )
        .await
        .expect("prompt response");

    assert!(
        response.error.is_none(),
        "/goal (no args) should be handled, got error: {:?}",
        response.error
    );

    let result = response.result.expect("should have result");
    let stop_reason = result
        .get("stopReason")
        .and_then(|v| v.as_str())
        .expect("should have stopReason");
    // /goal without args is treated as regular text, mock returns end_turn
    assert_eq!(
        stop_reason, "end_turn",
        "expected stopReason 'end_turn', got '{}'",
        stop_reason
    );
}
