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

fn extract_update_type(notification: &serde_json::Value) -> Option<String> {
    notification
        .get("params")
        .and_then(|p| p.get("update"))
        .and_then(|u| {
            let keys = [
                "sessionUpdate",
                "agent_message_chunk",
                "agent_thought_chunk",
                "user_message_chunk",
                "tool_call",
                "tool_call_update",
                "session_info_update",
                "plan",
            ];
            for key in keys {
                if u.get(key).is_some() {
                    return Some(key.to_string());
                }
            }
            None
        })
}

#[tokio::test]
async fn e2e_prompt_emits_session_update_notifications() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = handshake(&mut acp).await;

    let request_id = acp
        .send_prompt_request(&session_id, "Hello!")
        .expect("send prompt");

    let (notifications, response) = acp
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect notifications");

    assert!(
        response.error.is_none(),
        "prompt should succeed: {:?}",
        response.error
    );

    let update_notifications: Vec<_> = notifications
        .iter()
        .filter(|n| n.get("method").and_then(|v| v.as_str()) == Some("session/update"))
        .collect();

    assert!(
        !update_notifications.is_empty(),
        "should receive at least one session/update notification"
    );
}

#[tokio::test]
async fn e2e_prompt_notifications_contain_session_id() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = handshake(&mut acp).await;

    let request_id = acp
        .send_prompt_request(&session_id, "Test session_id in notifications")
        .expect("send prompt");

    let (notifications, _response) = acp
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect notifications");

    let session_updates: Vec<_> = notifications
        .iter()
        .filter(|n| n.get("method").and_then(|v| v.as_str()) == Some("session/update"))
        .collect();

    for notif in &session_updates {
        let notif_session = notif
            .get("params")
            .and_then(|p| p.get("sessionId"))
            .and_then(|v| v.as_str())
            .unwrap_or("missing");
        assert_eq!(
            notif_session, session_id,
            "notification sessionId should match"
        );
    }
}

#[tokio::test]
async fn e2e_prompt_response_has_stop_reason() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = handshake(&mut acp).await;

    let request_id = acp
        .send_prompt_request(&session_id, "Hello!")
        .expect("send prompt");

    let (_notifications, response) = acp
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect notifications");

    let stop_reason = response
        .result
        .as_ref()
        .and_then(|r| r.get("stopReason"))
        .and_then(|v| v.as_str());
    assert!(
        stop_reason.is_some(),
        "response should have stopReason"
    );
    let sr = stop_reason.unwrap();
    assert!(
        sr == "end_turn" || sr == "cancelled",
        "unexpected stopReason: {}",
        sr
    );
}

#[tokio::test]
async fn e2e_prompt_emits_agent_message_chunks() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = handshake(&mut acp).await;

    let request_id = acp
        .send_prompt_request(&session_id, "Tell me something")
        .expect("send prompt");

    let (notifications, _response) = acp
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect notifications");

    let update_types: Vec<String> = notifications
        .iter()
        .filter_map(|n| {
            if n.get("method").and_then(|v| v.as_str()) == Some("session/update") {
                extract_update_type(n)
            } else {
                None
            }
        })
        .collect();

    let has_message_or_thought = update_types.iter().any(|t| {
        t == "agent_message_chunk"
            || t == "agent_thought_chunk"
            || t == "user_message_chunk"
    });

    assert!(
        has_message_or_thought,
        "should have at least one message/thought chunk, got: {:?}",
        update_types
    );
}
