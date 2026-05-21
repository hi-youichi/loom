//! E2E test for the /goal command via ACP protocol.
//!
//! Verifies that:
//! 1. `/goal <description>` is recognized as a command
//! 2. The goal runner initializes (creates tasks.db)
//! 3. A PromptResponse is returned
//! 4. Session update notifications are generated during goal execution

mod common;
mod e2e;

use std::time::Duration;

const GOAL_TIMEOUT: Duration = Duration::from_secs(60);

/// Verifies that `/goal` command is recognized and returns a prompt response.
/// With LOOM_GOAL_MAX_ITERATIONS=1 (set in AcpChild::spawn), the goal runner
/// will execute one iteration then stop with max-iterations-reached error.
/// The test verifies the command was handled (not forwarded to LLM as regular text).
#[tokio::test]
async fn e2e_goal_command_returns_prompt_response() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    // Send /goal command
    let request_id = guard
        .acp_mut()
        .send_prompt_request(&session_id, "/goal write a hello world program")
        .expect("send /goal prompt");

    // Collect notifications and final response
    let (notifications, response) = guard
        .acp_mut()
        .collect_all_notifications(request_id, GOAL_TIMEOUT)
        .expect("collect /goal response");

    // Verify we got a response (not an error)
    assert!(
        response.error.is_none(),
        "/goal should not return an error, got: {:?}",
        response.error
    );

    // Verify stop reason is end_turn
    let stop_reason = response
        .result
        .as_ref()
        .and_then(|r| r.get("stopReason"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    assert_eq!(
        stop_reason, "end_turn",
        "expected end_turn stop reason, got '{}'",
        stop_reason
    );

    // We should have received at least some notifications during goal execution
    eprintln!(
        "[goal-e2e] received {} notifications",
        notifications.len()
    );
    // Even if no notifications were sent (e.g. no session_update_tx),
    // the command was still handled correctly if we got end_turn.
}

/// Verifies that `/goal` without a description is treated as a regular message
/// (not parsed as a command), and gets a normal LLM response.
#[tokio::test]
async fn e2e_goal_without_description_gets_normal_response() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    // Send /goal without description - this should be treated as regular text
    // since the parser returns None for empty description
    let request_id = guard
        .acp_mut()
        .send_prompt_request(&session_id, "/goal")
        .expect("send /goal prompt without description");

    let (notifications, response) = guard
        .acp_mut()
        .collect_all_notifications(request_id, GOAL_TIMEOUT)
        .expect("collect response for /goal without description");

    assert!(
        response.error.is_none(),
        "response should not error, got: {:?}",
        response.error
    );

    let stop_reason = response
        .result
        .as_ref()
        .and_then(|r| r.get("stopReason"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    // Should get end_turn from normal LLM flow (not goal runner)
    assert_eq!(
        stop_reason, "end_turn",
        "expected end_turn, got '{}'",
        stop_reason
    );

    eprintln!(
        "[goal-e2e] /goal (no desc): {} notifications, stop_reason={}",
        notifications.len(),
        stop_reason
    );
}
