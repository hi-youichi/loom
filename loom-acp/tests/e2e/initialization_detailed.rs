use std::time::Duration;

use crate::e2e;

#[test]
fn e2e_initialize_returns_capabilities() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    // Send initialize request
    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({
                "protocolVersion": 1,
            }),
            Duration::from_secs(10),
        )
        .expect("initialize response");

    // Should succeed
    assert!(response.error.is_none(), "initialize should succeed");
    let result = response.result.expect("should have result");

    // Verify protocolVersion
    let protocol_version = result
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .expect("should have protocolVersion");
    assert_eq!(protocol_version, "1", "protocolVersion should be '1'");

    // Verify agentCapabilities
    let capabilities = result
        .get("agentCapabilities")
        .and_then(|v| v.as_object())
        .expect("should have agentCapabilities");

    let required_capabilities = ["loadSession", "listTools", "promptCapabilities"];
    for cap in required_capabilities {
        assert!(
            capabilities.get(cap).is_some(),
            "should have capability: {}",
            cap
        );
    }

    // Verify agentInfo
    let agent_info = result
        .get("agentInfo")
        .and_then(|v| v.as_object())
        .expect("should have agentInfo");

    assert!(agent_info.get("name").is_some(), "agentInfo should have name");
    assert!(
        agent_info.get("version").is_some(),
        "agentInfo should have version"
    );
    assert!(
        agent_info.get("title").is_some(),
        "agentInfo should have title"
    );
}

#[test]
fn e2e_initialize_rejects_unsupported_version() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    // Send initialize with unsupported version
    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({
                "protocolVersion": 999,
            }),
            Duration::from_secs(10),
        )
        .expect("initialize response");

    // Should return error
    assert!(response.error.is_some(), "should return error for unsupported version");
    let error = response.error.as_ref().unwrap();
    
    // Should be Invalid Request error or contain version mismatch info
    assert!(
        error.code == -32600 || error.message.to_lowercase().contains("version"),
        "error should be Invalid Request or mention version: {}",
        error.message
    );
}

#[test]
fn e2e_authenticate_succeeds() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    // First initialize
    let init_response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({
                "protocolVersion": 1,
            }),
            Duration::from_secs(10),
        )
        .expect("initialize response");

    assert!(init_response.error.is_none(), "initialize should succeed");

    // Then authenticate (should succeed when no auth is configured)
    let auth_response = acp
        .send_request_and_wait(
            "authenticate",
            serde_json::json!({}),
            Duration::from_secs(10),
        )
        .expect("authenticate response");

    // Should succeed when no auth is configured
    assert!(
        auth_response.error.is_none(),
        "authenticate should succeed when no auth configured: {:?}",
        auth_response.error
    );
}