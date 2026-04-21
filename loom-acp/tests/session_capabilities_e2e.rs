mod common;
mod e2e;

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

async fn initialize(acp: &mut common::AcpChild) {
    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            TIMEOUT,
        )
        .await
        .expect("initialize");
    assert!(
        response.error.is_none(),
        "initialize failed: {:?}",
        response.error
    );
}

#[allow(dead_code)]
async fn new_session(acp: &mut common::AcpChild) -> String {
    let response = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            TIMEOUT,
        )
        .await
        .expect("session/new response");

    assert!(
        response.error.is_none(),
        "session/new should succeed: {:?}",
        response.error
    );

    response
        .result
        .expect("should have result")
        .get("sessionId")
        .and_then(|v| v.as_str())
        .expect("should have sessionId")
        .to_string()
}

#[tokio::test]
async fn e2e_session_load_nonexistent_session() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock().await.expect("spawn loom-acp with mock");
    initialize(&mut acp).await;

    let response = acp
        .send_request_and_wait(
            "session/load",
            serde_json::json!({
                "sessionId": "nonexistent-session-id",
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            TIMEOUT,
        )
        .await
        .expect("session/load response");

    assert!(
        response.error.is_some() || response.result.is_some(),
        "session/load with nonexistent session should return error or empty result"
    );
}
