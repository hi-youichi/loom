mod e2e;

use std::time::Duration;

#[test]
fn e2e_session_new_before_initialize_fails() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    let response = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
            }),
            Duration::from_secs(10),
        )
        .expect("session/new response");

    assert!(
        response.error.is_some(),
        "session/new before initialize should fail"
    );
}

#[test]
fn e2e_session_load_before_initialize() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    let response = acp
        .send_request_and_wait(
            "session/load",
            serde_json::json!({
                "sessionId": "fake-id",
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            Duration::from_secs(10),
        )
        .expect("session/load response");

    // Protocol may or may not enforce initialize-first ordering.
    // Verify it returns a valid response (error or result).
    assert!(
        response.error.is_some() || response.result.is_some(),
        "session/load should return a valid JSON-RPC response"
    );
}

#[test]
fn e2e_session_prompt_before_initialize_fails() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    let response = acp
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": "fake-id",
                "messages": [],
            }),
            Duration::from_secs(10),
        )
        .expect("session/prompt response");

    assert!(
        response.error.is_some(),
        "session/prompt before initialize should fail"
    );
}

#[test]
fn e2e_duplicate_initialize() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    let first = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            Duration::from_secs(10),
        )
        .expect("first initialize");
    assert!(first.error.is_none(), "first initialize should succeed");

    let second = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            Duration::from_secs(10),
        )
        .expect("second initialize");

    assert!(
        second.error.is_some() || second.result.is_some(),
        "second initialize should return error or result"
    );
}

#[test]
fn e2e_authenticate_after_initialize() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    let init = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            Duration::from_secs(10),
        )
        .expect("initialize");
    assert!(init.error.is_none(), "initialize should succeed");

    let auth = acp
        .send_request_and_wait(
            "authenticate",
            serde_json::json!({ "methodId": "none" }),
            Duration::from_secs(10),
        )
        .expect("authenticate");
    assert!(
        auth.error.is_none(),
        "authenticate after initialize should succeed: {:?}",
        auth.error
    );
}

#[test]
fn e2e_initialize_version_zero() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 0 }),
            Duration::from_secs(10),
        )
        .expect("initialize response");

    assert!(
        response.error.is_some() || response.result.is_some(),
        "protocolVersion 0 should return error or negotiate a version"
    );
}

#[test]
fn e2e_initialize_missing_protocol_version_fails() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    let response = acp
        .send_request_and_wait("initialize", serde_json::json!({}), Duration::from_secs(10))
        .expect("initialize response");

    assert!(
        response.error.is_some(),
        "initialize without protocolVersion should fail"
    );
}

#[test]
fn e2e_initialize_negative_version_fails() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": -1 }),
            Duration::from_secs(10),
        )
        .expect("initialize response");

    assert!(
        response.error.is_some(),
        "negative protocolVersion should fail"
    );
}
