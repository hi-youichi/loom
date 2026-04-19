//! E2E tests for Phase 3: Prompt Turn — conversation interaction with mock LLM.
//!
//! These tests use [`e2e::AcpChild::spawn_with_mock`] to start loom-acp with a mock
//! OpenAI-compatible HTTP server so no real API keys are needed.

mod e2e;

use std::time::Duration;


const TIMEOUT: Duration = Duration::from_secs(30);

/// Send a simple text prompt and verify the agent returns `end_turn`.
#[tokio::test]
async fn e2e_prompt_simple_text_response() {
    let (mut acp, _mock) = e2e::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = acp.handshake(TIMEOUT).expect("handshake");
    assert!(!session_id.is_empty(), "session_id should not be empty");

    // Send a prompt
    let prompt_response = acp
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
    let (mut acp, _mock) = e2e::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = acp.handshake(TIMEOUT).expect("handshake");

    // Send prompt
    let prompt_id = acp
        .send_request(
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": [{
                    "type": "text",
                    "text": "Say hello",
                }],
            }),
        )
        .expect("send prompt");

    // Collect notifications until we get the prompt response
    let start = std::time::Instant::now();
    let mut got_update = false;
    let mut got_response = false;

    while start.elapsed() < TIMEOUT && !got_response {
        let message = acp.read_message().expect("read message");

        if message.get("method").and_then(|v| v.as_str()) == Some("session/update") {
            got_update = true;
        }

        if message.get("id").and_then(|v| v.as_u64()) == Some(prompt_id) {
            got_response = true;
        }
    }

    assert!(got_response, "should receive prompt response");
    assert!(got_update, "should receive at least one session/update notification");
}
