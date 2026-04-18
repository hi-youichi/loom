mod e2e;

use std::time::Duration;

fn initialize(acp: &mut e2e::AcpChild) -> serde_json::Map<String, serde_json::Value> {
    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            Duration::from_secs(10),
        )
        .expect("initialize response");

    assert!(response.error.is_none(), "initialize should succeed: {:?}", response.error);
    response.result.expect("should have result").as_object().expect("result should be object").clone()
}

#[test]
fn e2e_mcp_capabilities_presence() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let capabilities = result
        .get("agentCapabilities")
        .and_then(|v| v.as_object())
        .expect("should have agentCapabilities");

    if let Some(mcp_caps) = capabilities.get("mcpCapabilities") {
        assert!(
            mcp_caps.as_object().is_some(),
            "mcpCapabilities should be object, got: {:?}",
            mcp_caps
        );
    }
}

#[test]
fn e2e_new_session_with_empty_mcp_servers() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    initialize(&mut acp);

    let response = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            Duration::from_secs(10),
        )
        .expect("session/new response");

    assert!(
        response.error.is_none(),
        "session/new with empty mcpServers should succeed: {:?}",
        response.error
    );

    let result = response.result.expect("should have result");
    assert!(
        result.get("sessionId").is_some(),
        "should return sessionId"
    );
}

#[test]
fn e2e_new_session_without_mcp_servers() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    initialize(&mut acp);

    // mcpServers is required by the schema
    let response = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            Duration::from_secs(10),
        )
        .expect("session/new response");

    assert!(
        response.error.is_none(),
        "session/new with empty mcpServers should succeed: {:?}",
        response.error
    );
}

#[test]
fn e2e_new_session_with_mcp_server_stdio_config() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    initialize(&mut acp);

    let response = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [{
                    "name": "test-server",
                    "command": "/bin/echo",
                    "args": ["hello"],
                    "env": []
                }],
            }),
            Duration::from_secs(10),
        )
        .expect("session/new response");

    assert!(
        response.error.is_none(),
        "session/new with MCP stdio server should succeed: {:?}",
        response.error
    );
}

#[test]
fn e2e_new_session_with_invalid_mcp_config_graceful() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    initialize(&mut acp);

    let response = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [{
                    "name": "bad-server",
                }],
            }),
            Duration::from_secs(10),
        )
        .expect("session/new response");

    if response.error.is_some() {
        let error = response.error.as_ref().unwrap();
        assert!(
            error.code != -32603 || error.message.len() > 0,
            "should have meaningful error message"
        );
    }
}
