mod common;
mod e2e;

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn e2e_prompt_returns_end_turn() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let response = guard.acp_mut()
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": "Hello!" }],
            }),
            TIMEOUT,
        )
        .await
        .expect("session/prompt response");

    assert!(
        response.error.is_none(),
        "prompt should succeed: {:?}",
        response.error
    );

    let result = response.result.expect("should have result");
    let stop_reason = result
        .get("stopReason")
        .and_then(|v| v.as_str())
        .expect("should have stopReason");
    assert_eq!(stop_reason, "end_turn");
}

#[tokio::test]
async fn e2e_multiple_prompts_all_succeed() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    for i in 0..3 {
        let response = guard.acp_mut()
            .send_request_and_wait(
                "session/prompt",
                serde_json::json!({
                    "sessionId": &session_id,
                    "prompt": [{ "type": "text", "text": format!("Prompt {}", i) }],
                }),
                TIMEOUT,
            )
            .await
            .expect("session/prompt response");

        assert!(
            response.error.is_none(),
            "prompt {} should succeed: {:?}",
            i,
            response.error
        );
    }
}

#[tokio::test]
async fn e2e_prompt_returns_valid_response_format() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let response = guard.acp_mut()
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": "Hello!" }],
            }),
            TIMEOUT,
        )
        .await
        .expect("session/prompt response");

    assert!(
        response.error.is_none(),
        "prompt should succeed: {:?}",
        response.error
    );

    let result = response.result.expect("should have result");
    assert!(result.get("stopReason").is_some(), "should have stopReason");
}
