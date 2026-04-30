mod common;
mod e2e;

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

async fn prompt_and_wait(
    guard: &mut common::process_pool::PooledAcpGuard,
    session_id: &str,
    text: &str,
) -> common::RpcResponse {
    guard.acp_mut().send_request_and_wait(
        "session/prompt",
        serde_json::json!({
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": text }],
        }),
        TIMEOUT,
    )
    .await
    .expect("session/prompt")
}

#[tokio::test]
async fn e2e_multi_turn_same_session_both_succeed() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let resp1 = prompt_and_wait(&mut guard, &session_id, "Hello!").await;
    assert!(resp1.error.is_none(), "first prompt error: {:?}", resp1.error);
    let sr1 = resp1
        .result
        .as_ref()
        .and_then(|r| r.get("stopReason"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    assert_eq!(sr1, "end_turn", "first prompt stop_reason: {}", sr1);

    let resp2 = prompt_and_wait(&mut guard, &session_id, "What did I just say?").await;
    assert!(resp2.error.is_none(), "second prompt error: {:?}", resp2.error);
    let sr2 = resp2
        .result
        .as_ref()
        .and_then(|r| r.get("stopReason"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    assert_eq!(sr2, "end_turn", "second prompt stop_reason: {}", sr2);
}

#[tokio::test]
async fn e2e_multi_turn_different_sessions_independent() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;

    let init = guard.acp_mut()
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            Duration::from_secs(10),
        )
        .await
        .expect("initialize");
    assert!(init.error.is_none());

    let s1 = guard.acp_mut()
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            TIMEOUT,
        )
        .await
        .expect("session/new 1");
    let sid1 = s1
        .result
        .unwrap()
        .get("sessionId")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let s2 = guard.acp_mut()
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            TIMEOUT,
        )
        .await
        .expect("session/new 2");
    let sid2 = s2
        .result
        .unwrap()
        .get("sessionId")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    assert_ne!(sid1, sid2, "two sessions should have different IDs");

    let r1 = prompt_and_wait(&mut guard, &sid1, "Session one hello").await;
    assert!(r1.error.is_none(), "session 1 prompt error: {:?}", r1.error);

    let r2 = prompt_and_wait(&mut guard, &sid2, "Session two hello").await;
    assert!(r2.error.is_none(), "session 2 prompt error: {:?}", r2.error);
}

#[tokio::test]
async fn e2e_prompt_unknown_session_returns_error() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;

    let init = guard.acp_mut()
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            Duration::from_secs(10),
        )
        .await
        .expect("initialize");
    assert!(init.error.is_none());

    let resp = guard.acp_mut()
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": "nonexistent-session-123",
                "prompt": [{ "type": "text", "text": "Hello" }],
            }),
            TIMEOUT,
        )
        .await
        .expect("prompt response");

    assert!(
        resp.error.is_some(),
        "prompt to unknown session should return error"
    );
}
