mod common;
mod e2e;

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(5);

fn assert_no_receiver_dropped_error(resp: &common::RpcResponse, context: &str) {
    if let Some(err) = &resp.error {
        assert_ne!(
            err.code, -32603,
            "{}: got Internal error (possible receiver dropped): {:?}",
            context, err
        );
    }
}

#[tokio::test]
#[ignore]
async fn e2e_prompt_sequential_no_receiver_dropped() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    for i in 0..3 {
        let resp = guard.acp_mut()
            .send_request_and_wait(
                "session/prompt",
                serde_json::json!({
                    "sessionId": &session_id,
                    "prompt": [{ "type": "text", "text": format!("Prompt {}", i) }],
                }),
                TIMEOUT,
            )
            .await
            .unwrap_or_else(|_| panic!("prompt {} response", i));

        assert!(
            resp.error.is_none(),
            "prompt {} should succeed, got error: {:?}",
            i,
            resp.error
        );
        assert_no_receiver_dropped_error(&resp, &format!("prompt {}", i));
    }
}

#[tokio::test]
#[ignore]
async fn e2e_prompt_response_not_internal_error() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let resp = guard.acp_mut()
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": &session_id,
                "prompt": [{ "type": "text", "text": "Hello" }],
            }),
            TIMEOUT,
        )
        .await
        .expect("prompt response");

    assert!(resp.error.is_none(), "prompt should succeed: {:?}", resp.error);

    let stop_reason = resp
        .result
        .as_ref()
        .and_then(|r| r.get("stopReason"))
        .and_then(|v| v.as_str())
        .expect("should have stopReason");
    assert_eq!(stop_reason, "end_turn");
}

#[tokio::test]
#[ignore]
async fn e2e_prompt_multi_session_no_receiver_dropped() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let sid1 = guard.new_session().await;
    let sid2 = guard.new_session().await;

    assert_ne!(sid1, sid2, "sessions should differ");

    let r1 = guard.acp_mut()
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": &sid1,
                "prompt": [{ "type": "text", "text": "Hello from session 1" }],
            }),
            TIMEOUT,
        )
        .await
        .expect("prompt session 1");

    assert!(r1.error.is_none(), "session 1 prompt error: {:?}", r1.error);
    assert_no_receiver_dropped_error(&r1, "session 1 prompt");

    let r2 = guard.acp_mut()
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": &sid2,
                "prompt": [{ "type": "text", "text": "Hello from session 2" }],
            }),
            TIMEOUT,
        )
        .await
        .expect("prompt session 2");

    assert!(r2.error.is_none(), "session 2 prompt error: {:?}", r2.error);
    assert_no_receiver_dropped_error(&r2, "session 2 prompt");
}

#[tokio::test]
#[ignore]
async fn e2e_prompt_with_notifications_no_receiver_dropped() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let request_id = guard.acp_mut()
        .send_prompt_request(&session_id, "Say hello")
        .expect("send prompt");

    let (notifications, response) = guard.acp_mut()
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect");

    assert!(
        response.error.is_none(),
        "prompt should succeed: {:?}",
        response.error
    );
    assert_no_receiver_dropped_error(&response, "prompt with notifications");

    let got_update = notifications
        .iter()
        .any(|msg| msg.get("method").and_then(|v| v.as_str()) == Some("session/update"));

    assert!(got_update, "should receive at least one session/update notification");
}
