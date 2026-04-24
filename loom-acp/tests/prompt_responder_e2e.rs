mod common;
mod e2e;

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

async fn handshake(acp: &mut common::AcpChild) -> String {
    let init = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            Duration::from_secs(10),
        )
        .await
        .expect("initialize");
    assert!(init.error.is_none(), "initialize failed: {:?}", init.error);

    let session = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            TIMEOUT,
        )
        .await
        .expect("session/new");
    assert!(session.error.is_none(), "session/new failed: {:?}", session.error);

    session
        .result
        .expect("should have result")
        .get("sessionId")
        .and_then(|v| v.as_str())
        .expect("should have sessionId")
        .to_string()
}

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
async fn e2e_prompt_sequential_no_receiver_dropped() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = handshake(&mut acp).await;

    for i in 0..5 {
        let resp = acp
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
async fn e2e_prompt_response_not_internal_error() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = handshake(&mut acp).await;

    let resp = acp
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
async fn e2e_prompt_multi_session_no_receiver_dropped() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let sid1 = handshake(&mut acp).await;
    let sid2 = handshake(&mut acp).await;

    assert_ne!(sid1, sid2, "sessions should differ");

    let r1 = acp
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

    let r2 = acp
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
async fn e2e_prompt_with_notifications_no_receiver_dropped() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = handshake(&mut acp).await;

    let request_id = acp
        .send_prompt_request(&session_id, "Say hello")
        .expect("send prompt");

    let (notifications, response) = acp
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
