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

fn extract_models_from_requests(requests: &Option<Vec<wiremock::Request>>) -> Vec<String> {
    requests
        .as_ref()
        .map(|reqs| {
            reqs
                .iter()
                .filter(|r| {
                    r.method == wiremock::http::Method::POST
                        && r.url.path().ends_with("/chat/completions")
                })
                .filter_map(|r| {
                    serde_json::from_slice::<serde_json::Value>(&r.body).ok()
                })
                .filter_map(|body| body.get("model").and_then(|m| m.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn e2e_title_generation_uses_light_tier_model() {
    let (mut acp, mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = handshake(&mut acp).await;

    let request_id = acp
        .send_prompt_request(&session_id, "Hello, help me with Rust")
        .expect("send prompt");

    let (_notifications, response) = acp
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect notifications");

    assert!(
        response.error.is_none(),
        "prompt should succeed: {:?}",
        response.error
    );

    let requests = mock.server.received_requests().await;
    let models_used = extract_models_from_requests(&requests);

    assert!(
        models_used.len() >= 2,
        "expected at least 2 LLM calls (main + title), got {} calls with models: {:?}",
        models_used.len(),
        models_used
    );

    let main_model = &models_used[0];
    let title_model = &models_used[1];

    assert_eq!(
        title_model, "test-model",
        "title generation should use light tier model, got: {}",
        title_model
    );

    assert_eq!(
        main_model, "test-model",
        "main prompt should use default model, got: {}",
        main_model
    );
}

#[tokio::test]
async fn e2e_title_generation_produces_session_info_update() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = handshake(&mut acp).await;

    let request_id = acp
        .send_prompt_request(&session_id, "Hello, help me with Rust")
        .expect("send prompt");

    let (notifications, response) = acp
        .collect_all_notifications_with_drain(request_id, TIMEOUT, Duration::from_secs(3))
        .expect("collect notifications");

    assert!(
        response.error.is_none(),
        "prompt should succeed: {:?}",
        response.error
    );

    let has_title_update = notifications.iter().any(|n| {
        if n.get("method").and_then(|v| v.as_str()) != Some("session/update") {
            return false;
        }
        let update_type = n.get("params")
            .and_then(|p| p.get("update"))
            .and_then(|u| u.get("sessionUpdate"))
            .and_then(|v| v.as_str())
            .unwrap_or("???");
        update_type == "session_info_update"
    });

    assert!(
        has_title_update,
        "expected session_info_update (title) notification"
    );
}

#[tokio::test]
async fn e2e_title_update_contains_non_empty_title() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = handshake(&mut acp).await;

    let request_id = acp
        .send_prompt_request(&session_id, "What is Rust?")
        .expect("send prompt");

    let (notifications, _response) = acp
        .collect_all_notifications_with_drain(request_id, TIMEOUT, Duration::from_secs(3))
        .expect("collect notifications");

    let titles: Vec<String> = notifications
        .iter()
        .filter_map(|n| {
            n.get("params")
                .and_then(|p| p.get("update"))
                .and_then(|u| {
                    if u.get("sessionUpdate").and_then(|v| v.as_str()) != Some("session_info_update") {
                        return None;
                    }
                    u.get("title").and_then(|t| t.as_str()).map(|s| s.to_string())
                })
        })
        .collect();

    let has_non_empty = titles.iter().any(|t| !t.is_empty());
    assert!(
        has_non_empty,
        "expected at least one non-empty title, got: {:?}",
        titles
    );
}

#[tokio::test]
async fn e2e_title_only_generated_on_first_prompt() {
    let (mut acp, mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");

    let session_id = handshake(&mut acp).await;

    let req1 = acp
        .send_prompt_request(&session_id, "First message")
        .expect("send first prompt");
    let (notifs1, _resp1) = acp
        .collect_all_notifications(req1, TIMEOUT)
        .expect("collect first");

    let title_count_1 = notifs1
        .iter()
        .filter(|n| {
            n.get("params")
                .and_then(|p| p.get("update"))
                .and_then(|u| u.get("sessionUpdate").and_then(|v| v.as_str()))
                == Some("session_info_update")
        })
        .count();

    let requests_after_first = mock.server.received_requests().await;
    let models_after_first = extract_models_from_requests(&requests_after_first);

    let req2 = acp
        .send_prompt_request(&session_id, "Second message")
        .expect("send second prompt");
    let (notifs2, _resp2) = acp
        .collect_all_notifications(req2, TIMEOUT)
        .expect("collect second");

    let title_count_2 = notifs2
        .iter()
        .filter(|n| {
            n.get("params")
                .and_then(|p| p.get("update"))
                .and_then(|u| u.get("sessionUpdate").and_then(|v| v.as_str()))
                == Some("session_info_update")
        })
        .count();

    let requests_after_second = mock.server.received_requests().await;
    let models_after_second = extract_models_from_requests(&requests_after_second);

    let new_models: Vec<_> = models_after_second
        [models_after_first.len()..]
        .to_vec();

    assert!(
        title_count_1 <= 1,
        "first prompt should generate at most 1 title update, got {}",
        title_count_1
    );
    assert_eq!(
        title_count_2, 0,
        "second prompt should not generate title updates"
    );
    assert!(
        new_models.len() == 1,
        "second prompt should only trigger 1 LLM call (no title), got {} calls: {:?}",
        new_models.len(),
        new_models
    );
}
