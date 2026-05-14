//! E2E tests for Phase 3: Prompt Turn — conversation interaction with mock LLM.
//!
//! These tests use [`common::process_pool::get_pool`] to start loom-acp with a mock
//! OpenAI-compatible HTTP server so no real API keys are needed.

mod common;
mod e2e;

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);

/// Send a simple text prompt and verify the agent returns `end_turn`.
#[tokio::test]
async fn e2e_prompt_simple_text_response() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;
    assert!(!session_id.is_empty(), "session_id should not be empty");

    // Send a prompt
    let prompt_response = guard.acp_mut()
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": [{
                    "type": "text",
                    "text": "Hello, say hi!",
                }],
            }),
            TIMEOUT,
        )
        .await
        .expect("session/prompt response");

    // Should succeed (no error)
    assert!(
        prompt_response.error.is_none(),
        "prompt should succeed, got error: {:?}",
        prompt_response.error
    );

    // Should have a result with stopReason
    let result = prompt_response.result.expect("should have result");
    let stop_reason = result
        .get("stopReason")
        .and_then(|v| v.as_str())
        .expect("should have stopReason");
    assert_eq!(
        stop_reason, "end_turn",
        "expected stopReason 'end_turn', got '{}'",
        stop_reason
    );
}

/// Send a prompt and verify that session/update notifications are emitted.
#[tokio::test]
async fn e2e_prompt_emits_update_notifications() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let request_id = guard.acp_mut().send_prompt_request(&session_id, "Say hello").expect("send prompt");

    let (notifications, response) = guard.acp_mut()
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect");

    assert!(
        response.error.is_none(),
        "prompt should succeed: {:?}",
        response.error
    );

    let got_update = notifications
        .iter()
        .any(|msg| msg.get("method").and_then(|v| v.as_str()) == Some("session/update"));

    assert!(got_update, "should receive at least one session/update notification");
}
