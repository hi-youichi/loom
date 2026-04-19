//! E2E tests for prompt capabilities

mod e2e;

use std::time::Duration;

fn handshake(acp: &mut e2e::AcpChild) -> String {
    let init = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            Duration::from_secs(10),
        )
        .expect("initialize");
    assert!(init.error.is_none(), "initialize failed: {:?}", init.error);

    let sess = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            Duration::from_secs(10),
        )
        .expect("session/new");
    assert!(sess.error.is_none(), "session/new failed: {:?}", sess.error);

    sess.result
        .expect("should have result")
        .get("sessionId")
        .and_then(|v| v.as_str())
        .expect("should have sessionId")
        .to_string()
}

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
    assert!(
        response.error.is_none(),
        "initialize with v1 should succeed"
    );
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
    assert!(
        prompt_caps.get("image").is_some(),
        "image should be present"
    );
    assert!(
        prompt_caps.get("audio").is_some(),
        "audio should be present"
    );
}

#[test]
fn e2e_prompt_with_embedded_resource() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let session_id = handshake(&mut acp);

    let response = acp
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": [{
                    "type": "text",
                    "text": "What is in this resource?"
                }, {
                    "type": "resource",
                    "resource": {
                        "uri": "file:///tmp/test.txt",
                        "mimeType": "text/plain",
                        "text": "Hello from embedded resource"
                    }
                }],
            }),
            Duration::from_secs(30),
        )
        .expect("session/prompt response");

    assert!(
        response.error.is_none(),
        "prompt with embedded resource should not return protocol error: {:?}",
        response.error
    );
}

#[test]
fn e2e_prompt_with_image_block() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let session_id = handshake(&mut acp);

    let tiny_png_base64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    let response = acp
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": [{
                    "type": "text",
                    "text": "Describe this image"
                }, {
                    "type": "image",
                    "data": tiny_png_base64,
                    "mimeType": "image/png"
                }],
            }),
            Duration::from_secs(30),
        )
        .expect("session/prompt response");

    assert!(
        response.error.is_none(),
        "prompt with image block should not return protocol error: {:?}",
        response.error
    );
}

#[test]
fn e2e_prompt_with_audio_block() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let session_id = handshake(&mut acp);

    let fake_audio_base64 = "UklGRiQAAABXQVZFZm10IBAAAAABAAEARKwAAIhYAQACABAAZGF0YQAAAAA=";

    let response = acp
        .send_request_and_wait(
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": [{
                    "type": "text",
                    "text": "Transcribe this audio"
                }, {
                    "type": "audio",
                    "data": fake_audio_base64,
                    "mimeType": "audio/wav"
                }],
            }),
            Duration::from_secs(30),
        )
        .expect("session/prompt response");

    assert!(
        response.error.is_none(),
        "prompt with audio block should not return protocol error: {:?}",
        response.error
    );
}
