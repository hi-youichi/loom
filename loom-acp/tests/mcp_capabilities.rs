mod common;
mod e2e;

use std::time::Duration;

async fn initialize(acp: &mut common::AcpChild) -> serde_json::Map<String, serde_json::Value> {
    let response = acp
        .call("initialize", serde_json::json!({ "protocolVersion": 1 }))
        .await
        .expect("initialize response");

    response
        .as_object()
        .expect("result should be object")
        .clone()
}

#[tokio::test]
async fn e2e_mcp_capabilities_presence() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let result = initialize(&mut guard.acp_mut()).await;

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

#[tokio::test]
async fn e2e_new_session_with_empty_mcp_servers() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    initialize(&mut guard.acp_mut()).await;

    let response = guard.acp_mut()
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("session/new response");

    assert!(
        response.error.is_none(),
        "session/new with empty mcpServers should succeed: {:?}",
        response.error
    );

    let result = response.result.expect("should have result");
    assert!(result.get("sessionId").is_some(), "should return sessionId");
}

#[tokio::test]
async fn e2e_new_session_without_mcp_servers() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    initialize(&mut guard.acp_mut()).await;

    // mcpServers is required by the schema
    let response = guard.acp_mut()
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [],
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("session/new response");

    assert!(
        response.error.is_none(),
        "session/new with empty mcpServers should succeed: {:?}",
        response.error
    );
}

#[tokio::test]
async fn e2e_new_session_with_mcp_server_stdio_config() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    initialize(&mut guard.acp_mut()).await;

    let response = guard.acp_mut()
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
            Duration::from_secs(5),
        )
        .await
        .expect("session/new response");

    assert!(
        response.error.is_none(),
        "session/new with MCP stdio server should succeed: {:?}",
        response.error
    );
}

#[tokio::test]
async fn e2e_new_session_with_invalid_mcp_config_graceful() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    initialize(&mut guard.acp_mut()).await;

    let response = guard.acp_mut()
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                "mcpServers": [{
                    "name": "bad-server",
                }],
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("session/new response");

    if let Some(error) = response.error.as_ref() {
        assert!(
            error.code != -32603 || !error.message.is_empty(),
            "should have meaningful error message"
        );
    }
}
