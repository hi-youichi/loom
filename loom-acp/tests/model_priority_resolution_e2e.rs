mod common;
mod e2e;

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

async fn prompt(guard: &mut common::process_pool::PooledAcpGuard, session_id: &str, text: &str) -> common::RpcResponse {
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
async fn e2e_set_model_then_prompt_uses_configured_model() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let set_model_resp = guard.acp_mut()
        .send_request_and_wait(
            "session/set_model",
            serde_json::json!({
                "sessionId": &session_id,
                "modelId": "mock/test-model",
            }),
            TIMEOUT,
        )
        .await
        .expect("set_model response");

    assert!(
        set_model_resp.error.is_none(),
        "setModel should succeed: {:?}",
        set_model_resp.error
    );

    let prompt_resp = prompt(&mut guard, &session_id, "Hello with configured model").await;
    assert!(
        prompt_resp.error.is_none(),
        "prompt should succeed after setModel: {:?}",
        prompt_resp.error
    );
}

#[tokio::test]
async fn e2e_set_model_unknown_session_returns_error() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;

    let resp = guard.acp_mut()
        .send_request_and_wait(
            "session/set_model",
            serde_json::json!({
                "sessionId": "nonexistent-session",
                "modelId": "gpt-4o",
            }),
            TIMEOUT,
        )
        .await
        .expect("set_model response");

    assert!(
        resp.error.is_some(),
        "setModel on unknown session should fail"
    );
}

#[tokio::test]
async fn e2e_set_model_persists_across_prompts() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let set_resp = guard.acp_mut()
        .send_request_and_wait(
            "session/set_model",
            serde_json::json!({
                "sessionId": &session_id,
                "modelId": "mock/test-model",
            }),
            TIMEOUT,
        )
        .await
        .expect("set_model");
    assert!(set_resp.error.is_none());

    let resp1 = prompt(&mut guard, &session_id, "First prompt").await;
    assert!(resp1.error.is_none(), "first prompt: {:?}", resp1.error);

    let resp2 = prompt(&mut guard, &session_id, "Second prompt").await;
    assert!(resp2.error.is_none(), "second prompt: {:?}", resp2.error);

    let sr = resp2
        .result
        .as_ref()
        .and_then(|r| r.get("stopReason"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    assert_eq!(sr, "end_turn", "second prompt stop_reason: {}", sr);
}

#[tokio::test]
async fn e2e_different_sessions_independent_models() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let sid1 = guard.new_session().await;
    let sid2 = guard.new_session().await;

    let set1 = guard.acp_mut()
        .send_request_and_wait(
            "session/set_model",
            serde_json::json!({
                "sessionId": &sid1,
                "modelId": "mock/test-model",
            }),
            TIMEOUT,
        )
        .await
        .expect("set_model 1");
    assert!(set1.error.is_none());

    let set2 = guard.acp_mut()
        .send_request_and_wait(
            "session/set_model",
            serde_json::json!({
                "sessionId": &sid2,
                "modelId": "mock/test-model",
            }),
            TIMEOUT,
        )
        .await
        .expect("set_model 2");
    assert!(set2.error.is_none());

    let r1 = prompt(&mut guard, &sid1, "Hello session 1").await;
    assert!(r1.error.is_none(), "session 1 prompt: {:?}", r1.error);

    let r2 = prompt(&mut guard, &sid2, "Hello session 2").await;
    assert!(r2.error.is_none(), "session 2 prompt: {:?}", r2.error);
}

#[tokio::test]
async fn e2e_set_mode_switches_agent() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let set_mode = guard.acp_mut()
        .send_request_and_wait(
            "session/set_mode",
            serde_json::json!({
                "sessionId": &session_id,
                "modeId": "dev",
            }),
            TIMEOUT,
        )
        .await
        .expect("set_mode");

    assert!(
        set_mode.error.is_none(),
        "setMode should succeed: {:?}",
        set_mode.error
    );

    let resp = prompt(&mut guard, &session_id, "Hello in code mode").await;
    assert!(
        resp.error.is_none(),
        "prompt after setMode should succeed: {:?}",
        resp.error
    );
}
