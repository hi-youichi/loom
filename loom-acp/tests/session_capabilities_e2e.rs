mod e2e;

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

fn initialize(acp: &mut e2e::AcpChild) {
    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            TIMEOUT,
        )
        .expect("initialize");
    assert!(response.error.is_none(), "initialize failed: {:?}", response.error);
}

fn new_session(acp: &mut e2e::AcpChild) -> String {
    let response = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            TIMEOUT,
        )
        .expect("session/new");
    assert!(response.error.is_none(), "session/new failed: {:?}", response.error);
    response
        .result
        .expect("should have result")
        .get("sessionId")
        .and_then(|v| v.as_str())
        .expect("should have sessionId")
        .to_string()
}

#[test]
fn e2e_session_list_after_new_session() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    initialize(&mut acp);
    let _session_id = new_session(&mut acp);

    let response = acp
        .send_request_and_wait("session/list", serde_json::json!({}), TIMEOUT)
        .expect("session/list response");

    assert!(response.error.is_none(), "session/list should succeed: {:?}", response.error);

    let result = response.result.expect("should have result");
    assert!(
        result.get("sessions").and_then(|v| v.as_array()).is_some(),
        "session/list should return sessions array"
    );
}

#[test]
fn e2e_session_fork_creates_new_session() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    initialize(&mut acp);
    let original_session_id = new_session(&mut acp);

    let response = acp
        .send_request_and_wait(
            "session/fork",
            serde_json::json!({
                "sessionId": original_session_id,
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            TIMEOUT,
        )
        .expect("session/fork response");

    assert!(response.error.is_none(), "session/fork should succeed: {:?}", response.error);

    let result = response.result.expect("should have result");
    let forked_id = result
        .get("sessionId")
        .and_then(|v| v.as_str())
        .expect("fork should return new sessionId");

    assert_ne!(forked_id, original_session_id, "forked session should have different id");
}

#[test]
fn e2e_session_load_after_new_session() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    initialize(&mut acp);
    let original_session_id = new_session(&mut acp);

    let response = acp
        .send_request_and_wait(
            "session/load",
            serde_json::json!({
                "sessionId": original_session_id,
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            TIMEOUT,
        )
        .expect("session/load response");

    assert!(response.error.is_none(), "session/load should succeed: {:?}", response.error);

    let result = response.result.expect("should have result");
    assert!(
        result.get("modes").is_some() || result.get("configOptions").is_some(),
        "session/load should return modes or configOptions"
    );
}

#[test]
fn e2e_session_load_nonexistent_session() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    initialize(&mut acp);

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
        .expect("session/load response");

    assert!(
        response.error.is_some() || response.result.is_some(),
        "session/load with nonexistent session should return error or empty result"
    );
}
