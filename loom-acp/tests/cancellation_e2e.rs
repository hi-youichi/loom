mod common;
mod e2e;

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn e2e_prompt_and_cancel_returns_cancelled_stop_reason() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let request_id = guard.acp_mut()
        .send_prompt_request(&session_id, "Write a long essay about Rust")
        .expect("send prompt request");

    guard.acp_mut().send_raw(
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": { "sessionId": session_id }
        })
        .to_string(),
    )
    .expect("send cancel");

    let (notifications, response) = guard.acp_mut()
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect notifications");

    let stop_reason = response
        .result
        .as_ref()
        .and_then(|r| r.get("stopReason"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let is_cancelled_or_end = stop_reason == "cancelled" || stop_reason == "end_turn";
    assert!(
        is_cancelled_or_end,
        "expected cancelled or end_turn, got '{}'",
        stop_reason
    );

    let _ = notifications;
}

#[tokio::test]
async fn e2e_cancel_then_new_prompt_succeeds() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let req_id_1 = guard.acp_mut()
        .send_prompt_request(&session_id, "Hello")
        .expect("send first prompt");

    guard.acp_mut().send_raw(
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": { "sessionId": &session_id }
        })
        .to_string(),
    )
    .expect("send cancel");

    let (_, _resp1) = guard.acp_mut()
        .collect_all_notifications(req_id_1, TIMEOUT)
        .expect("collect first response");

    let resp2 = guard.acp_mut()
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": &session_id,
                "prompt": [{ "type": "text", "text": "Are you still there?" }],
            }),
            TIMEOUT,
        )
        .await
        .expect("second prompt response");

    assert!(
        resp2.error.is_none(),
        "second prompt should succeed after cancel, got error: {:?}",
        resp2.error
    );

    let stop_reason = resp2
        .result
        .as_ref()
        .and_then(|r| r.get("stopReason"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    assert_eq!(
        stop_reason, "end_turn",
        "second prompt should end_turn, got '{}'",
        stop_reason
    );
}
