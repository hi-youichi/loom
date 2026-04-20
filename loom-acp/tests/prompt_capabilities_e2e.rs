//! E2E tests for prompt capabilities

mod common;
mod e2e;

use std::time::Duration;
use serde_json::json;

async fn handshake(acp: &mut common::AcpChild) -> String {
    let init = acp
        .send_request_and_wait(
            "initialize",
            json!({ "protocolVersion": 1 }),
            Duration::from_secs(10),
        )
        .await
        .expect("initialize");
    assert!(init.error.is_none(), "initialize failed: {:?}", init.error);

    let response = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            Duration::from_secs(10),
        )
        .await.expect("session/new response");

    assert!(response.error.is_none(), "session/new should succeed");
    let result = response.result.expect("should have result");

    result
        .get("sessionId")
        .and_then(|v: &serde_json::Value| v.as_str())
        .expect("should have sessionId")
        .to_string()
}

#[tokio::test]
async fn e2e_prompt_capabilities_type_validation() {
    let mut acp = common::AcpChild::spawn(None).expect("spawn loom-acp");

    // Send initialize request
    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({
                "protocolVersion": 1,
            }),
            Duration::from_secs(10),
        )
        .await.expect("initialize response");

    assert!(response.error.is_none(), "initialize should succeed");
    let result = response.result.expect("should have result");

    // Get prompt capabilities
    let prompt_caps = result
        .get("agentCapabilities")
        .and_then(|v: &serde_json::Value| v.get("promptCapabilities"))
        .and_then(|v: &serde_json::Value| v.as_object())
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

#[tokio::test]
async fn e2e_prompt_capabilities_no_extra_fields() {
    let mut acp = common::AcpChild::spawn(None).expect("spawn loom-acp");

    // Send initialize request
    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({
                "protocolVersion": 1,
            }),
            Duration::from_secs(10),
        )
        .await.expect("initialize response");

    assert!(response.error.is_none(), "initialize should succeed");
    let result = response.result.expect("should have result");

    // Get prompt capabilities
    let prompt_caps = result
        .get("agentCapabilities")
        .and_then(|v: &serde_json::Value| v.get("promptCapabilities"))
        .and_then(|v: &serde_json::Value| v.as_object())
        .expect("should have promptCapabilities");

    // Verify only expected fields exist
    let expected_fields = ["embeddedContext", "image", "audio"];
    for (key, _value) in prompt_caps.iter() {
        let key_str: &str = key.as_str();
        assert!(
            expected_fields.contains(&key_str),
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

#[tokio::test]
async fn e2e_prompt_capabilities_with_different_protocol_versions() {
    // Test with protocol version 1 (current supported version)
    let mut acp = common::AcpChild::spawn(None).expect("spawn loom-acp");

    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({
                "protocolVersion": 1,
            }),
            Duration::from_secs(10),
        )
        .await.expect("initialize response");

    // Should succeed with supported version
    assert!(
        response.error.is_none(),
        "initialize with v1 should succeed"
    );
    let result = response.result.expect("should have result");

    // Verify prompt capabilities are present
    let prompt_caps = result
        .get("agentCapabilities")
        .and_then(|v: &serde_json::Value| v.get("promptCapabilities"))
        .and_then(|v: &serde_json::Value| v.as_object())
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

#[tokio::test]
#[ignore = "requires configured LLM API access"]
async fn e2e_prompt_with_embedded_resource() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock().await.expect("spawn loom-acp with mock");
    let session_id = handshake(&mut acp).await;

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
        .await
        .expect("session/prompt response");

    assert!(
        response.error.is_none(),
        "prompt with embedded resource should not return protocol error: {:?}",
        response.error
    );
}

#[tokio::test]
#[ignore = "requires configured LLM API access"]
async fn e2e_prompt_with_image_block() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock().await.expect("spawn loom-acp with mock");
    let session_id = handshake(&mut acp).await;

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
        .await
        .expect("session/prompt response");

    assert!(
        response.error.is_none(),
        "prompt with image block should not return protocol error: {:?}",
        response.error
    );
}

#[tokio::test]
#[ignore = "requires configured LLM API access"]
async fn e2e_prompt_with_audio_block() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock().await.expect("spawn loom-acp with mock");
    let session_id = handshake(&mut acp).await;

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
        .await
        .expect("session/prompt response");

    assert!(
        response.error.is_none(),
        "prompt with audio block should not return protocol error: {:?}",
        response.error
    );
}

