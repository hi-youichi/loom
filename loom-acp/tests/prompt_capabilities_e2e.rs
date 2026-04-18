//! E2E tests for prompt capabilities

mod e2e;

use std::time::Duration;

#[test]
fn e2e_prompt_capabilities_structure() {
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

    // Verify promptCapabilities structure exists
    let capabilities = result
        .get("agentCapabilities")
        .and_then(|v| v.as_object())
        .expect("should have agentCapabilities");

    let prompt_caps = capabilities
        .get("promptCapabilities")
        .and_then(|v| v.as_object())
        .expect("should have promptCapabilities");

    // Verify all expected prompt capability fields exist
    assert!(
        prompt_caps.get("embeddedContext").is_some(),
        "should have embeddedContext capability"
    );
    assert!(
        prompt_caps.get("image").is_some(),
        "should have image capability"
    );
    assert!(
        prompt_caps.get("audio").is_some(),
        "should have audio capability"
    );
}

#[test]
fn e2e_prompt_capabilities_boolean_values() {
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

    assert!(response.error.is_none(), "initialize should succeed");
    let result = response.result.expect("should have result");

    // Get prompt capabilities
    let prompt_caps = result
        .get("agentCapabilities")
        .and_then(|v| v.get("promptCapabilities"))
        .and_then(|v| v.as_object())
        .expect("should have promptCapabilities");

    // Verify all capabilities are boolean true
    assert_eq!(
        prompt_caps.get("embeddedContext").and_then(|v| v.as_bool()),
        Some(true),
        "embeddedContext should be true"
    );
    assert_eq!(
        prompt_caps.get("image").and_then(|v| v.as_bool()),
        Some(true),
        "image should be true"
    );
    assert_eq!(
        prompt_caps.get("audio").and_then(|v| v.as_bool()),
        Some(true),
        "audio should be true"
    );
}

#[test]
fn e2e_prompt_capabilities_type_validation() {
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

    assert!(response.error.is_none(), "initialize should succeed");
    let result = response.result.expect("should have result");

    // Get prompt capabilities
    let prompt_caps = result
        .get("agentCapabilities")
        .and_then(|v| v.get("promptCapabilities"))
        .and_then(|v| v.as_object())
        .expect("should have promptCapabilities");

    // Verify each capability is a boolean value
    for cap in ["embeddedContext", "image", "audio"] {
        let value = prompt_caps.get(cap).expect(&format!("should have {}", cap));
        assert!(
            value.is_boolean(),
            "{} should be a boolean value, got: {:?}",
            cap,
            value
        );
    }
}

#[test]
fn e2e_prompt_capabilities_no_extra_fields() {
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

    assert!(response.error.is_none(), "initialize should succeed");
    let result = response.result.expect("should have result");

    // Get prompt capabilities
    let prompt_caps = result
        .get("agentCapabilities")
        .and_then(|v| v.get("promptCapabilities"))
        .and_then(|v| v.as_object())
        .expect("should have promptCapabilities");

    // Verify only expected fields exist
    let expected_fields = ["embeddedContext", "image", "audio"];
    for (key, _value) in prompt_caps.iter() {
        assert!(
            expected_fields.contains(&key.as_str()),
            "unexpected field in promptCapabilities: {}",
            key
        );
    }

    // Verify all expected fields are present
    assert_eq!(
        prompt_caps.len(),
        expected_fields.len(),
        "promptCapabilities should have exactly {} fields",
        expected_fields.len()
    );
}

#[test]
fn e2e_prompt_capabilities_with_different_protocol_versions() {
    // Test with protocol version 1 (current supported version)
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({
                "protocolVersion": 1,
            }),
            Duration::from_secs(10),
        )
        .expect("initialize response");

    // Should succeed with supported version
    assert!(response.error.is_none(), "initialize with v1 should succeed");
    let result = response.result.expect("should have result");

    // Verify prompt capabilities are present
    let prompt_caps = result
        .get("agentCapabilities")
        .and_then(|v| v.get("promptCapabilities"))
        .and_then(|v| v.as_object())
        .expect("should have promptCapabilities");

    assert!(
        prompt_caps.get("embeddedContext").is_some(),
        "embeddedContext should be present"
    );
    assert!(prompt_caps.get("image").is_some(), "image should be present");
    assert!(prompt_caps.get("audio").is_some(), "audio should be present");
}