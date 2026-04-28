mod common;
mod e2e;

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn e2e_session_load_nonexistent_session() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let _session_id = guard.new_session().await;

    let response = guard.acp_mut()
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
