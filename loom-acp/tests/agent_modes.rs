//! ACP Agent integration tests for session modes feature.

use agent_client_protocol::{
    Agent, LoadSessionRequest, NewSessionRequest, SetSessionConfigOptionRequest,
    SetSessionModeRequest,
};
use loom_acp::LoomAcpAgent;
use std::path::PathBuf;

fn make_new_session_request() -> NewSessionRequest {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    NewSessionRequest::new(cwd)
}

fn extract_mode_ids_from_json(json: &serde_json::Value) -> Vec<String> {
    let modes_field = json.get("modes");
    if modes_field.is_none() {
        eprintln!(
            "JSON has no 'modes' field. Keys: {:?}",
            json.as_object().map(|o| o.keys().collect::<Vec<_>>())
        );
        return vec![];
    }
    let modes_obj = modes_field.unwrap();
    if let Some(arr) = modes_obj.get("availableModes").and_then(|m| m.as_array()) {
        return arr
            .iter()
            .filter_map(|entry| entry.get("id").and_then(|id| id.as_str().map(String::from)))
            .collect();
    }
    if let Some(arr) = modes_obj.as_array() {
        return arr
            .iter()
            .filter_map(|entry| entry.get("id").and_then(|id| id.as_str().map(String::from)))
            .collect();
    }
    eprintln!(
        "Unexpected modes structure: {}",
        serde_json::to_string_pretty(modes_obj).unwrap()
    );
    vec![]
}

fn extract_current_mode_id(json: &serde_json::Value) -> Option<String> {
    json.get("modes")
        .and_then(|m| m.get("currentModeId"))
        .and_then(|id| id.as_str().map(String::from))
}

#[tokio::test]
async fn test_new_session_returns_modes_with_ask_and_default() {
    let agent = LoomAcpAgent::new();
    let response = agent.new_session(make_new_session_request()).await.unwrap();
    let json = serde_json::to_value(&response).unwrap();

    let modes = extract_mode_ids_from_json(&json);
    assert!(
        modes.contains(&"ask".to_string()),
        "modes should contain 'ask', got: {:?}",
        modes
    );
    assert!(
        modes.contains(&"dev".to_string()),
        "modes should contain 'dev', got: {:?}",
        modes
    );
}

#[tokio::test]
async fn test_new_session_default_mode_is_dev() {
    let agent = LoomAcpAgent::new();
    let response = agent.new_session(make_new_session_request()).await.unwrap();
    let json = serde_json::to_value(&response).unwrap();

    let current = extract_current_mode_id(&json);
    assert_eq!(current, Some("dev".to_string()));
}

#[tokio::test]
async fn test_set_session_mode_and_load_preserves_mode() {
    let agent = LoomAcpAgent::new();
    let ns_response = agent.new_session(make_new_session_request()).await.unwrap();
    let session_id = ns_response.session_id.clone();

    let set_request: SetSessionModeRequest = serde_json::from_value(serde_json::json!({
        "sessionId": session_id.to_string(),
        "modeId": "dev"
    }))
    .unwrap();
    agent.set_session_mode(set_request).await.unwrap();

    let load_request: LoadSessionRequest = serde_json::from_value(serde_json::json!({
        "sessionId": session_id.to_string(),
        "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
        "mcpServers": []
    }))
    .unwrap();

    let load_response = agent.load_session(load_request).await.unwrap();
    let json = serde_json::to_value(&load_response).unwrap();
    let current = extract_current_mode_id(&json);
    assert_eq!(
        current,
        Some("dev".to_string()),
        "load_session should preserve the mode set via set_session_mode"
    );
}

#[tokio::test]
async fn test_set_session_mode_rejects_unknown_mode() {
    let agent = LoomAcpAgent::new();
    let ns_response = agent.new_session(make_new_session_request()).await.unwrap();
    let session_id = ns_response.session_id.clone();

    let set_request: SetSessionModeRequest = serde_json::from_value(serde_json::json!({
        "sessionId": session_id.to_string(),
        "modeId": "nonexistent-mode"
    }))
    .unwrap();

    let result = agent.set_session_mode(set_request).await;
    assert!(result.is_err(), "expected error for unknown mode");
}

#[tokio::test]
async fn test_set_session_mode_rejects_unknown_session() {
    let agent = LoomAcpAgent::new();

    let set_request: SetSessionModeRequest = serde_json::from_value(serde_json::json!({
        "sessionId": "nonexistent-session",
        "modeId": "ask"
    }))
    .unwrap();

    let result = agent.set_session_mode(set_request).await;
    assert!(result.is_err(), "expected error for unknown session");
}

#[tokio::test]
async fn test_load_session_new_entry_defaults_to_dev() {
    let agent = LoomAcpAgent::new();
    let ns_response = agent.new_session(make_new_session_request()).await.unwrap();
    let session_id = ns_response.session_id.clone();

    let load_request: LoadSessionRequest = serde_json::from_value(serde_json::json!({
        "sessionId": session_id.to_string(),
        "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
        "mcpServers": []
    }))
    .unwrap();
    let load_response = agent.load_session(load_request).await.unwrap();
    let json = serde_json::to_value(&load_response).unwrap();
    let current = extract_current_mode_id(&json);
    assert_eq!(
        current,
        Some("dev".to_string()),
        "load_session new entry should default to dev mode"
    );
}

#[tokio::test]
async fn test_load_session_modes_list_contains_builtins() {
    let agent = LoomAcpAgent::new();
    let fake_session_id = "session-load-test-002";

    let load_request: LoadSessionRequest = serde_json::from_value(serde_json::json!({
        "sessionId": fake_session_id,
        "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
        "mcpServers": []
    }))
    .unwrap();

    let load_response = agent.load_session(load_request).await.unwrap();
    let json = serde_json::to_value(&load_response).unwrap();
    let modes = extract_mode_ids_from_json(&json);

    assert!(
        modes.contains(&"ask".to_string()),
        "load_session modes should contain 'ask', got: {:?}",
        modes
    );
    assert!(
        modes.contains(&"dev".to_string()),
        "load_session modes should contain 'dev', got: {:?}",
        modes
    );
}

#[tokio::test]
async fn test_set_session_config_option_mode_switches_mode_and_returns_mode_first() {
    let agent = LoomAcpAgent::new();
    let ns_response = agent.new_session(make_new_session_request()).await.unwrap();
    let session_id = ns_response.session_id.clone();

    let set_config_request: SetSessionConfigOptionRequest =
        serde_json::from_value(serde_json::json!({
            "sessionId": session_id.to_string(),
            "configId": "mode",
            "value": "dev"
        }))
        .unwrap();

    let set_response = agent
        .set_session_config_option(set_config_request)
        .await
        .unwrap();
    let response_json = serde_json::to_value(&set_response).unwrap();
    let options = response_json["configOptions"]
        .as_array()
        .expect("configOptions should be an array");
    assert!(
        options.len() >= 2,
        "configOptions should include mode and model, got: {}",
        response_json
    );
    assert_eq!(options[0]["id"], "mode");
    assert_eq!(options[0]["currentValue"], "dev");
    assert_eq!(options[1]["id"], "model");

    let load_request: LoadSessionRequest = serde_json::from_value(serde_json::json!({
        "sessionId": session_id.to_string(),
        "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
        "mcpServers": []
    }))
    .unwrap();
    let load_response = agent.load_session(load_request).await.unwrap();
    let load_json = serde_json::to_value(&load_response).unwrap();
    assert_eq!(extract_current_mode_id(&load_json), Some("dev".to_string()));
}

#[tokio::test]
async fn test_set_session_config_option_mode_accepts_typed_value_payload() {
    let agent = LoomAcpAgent::new();
    let ns_response = agent.new_session(make_new_session_request()).await.unwrap();
    let session_id = ns_response.session_id.clone();

    // Simulate ACP typed payload used by newer clients.
    let set_config_request: SetSessionConfigOptionRequest =
        serde_json::from_value(serde_json::json!({
            "sessionId": session_id.to_string(),
            "configId": "mode",
            "type": "value_id",
            "value": "dev"
        }))
        .unwrap();

    let set_response = agent
        .set_session_config_option(set_config_request)
        .await
        .unwrap();
    let response_json = serde_json::to_value(&set_response).unwrap();
    let options = response_json["configOptions"]
        .as_array()
        .expect("configOptions should be an array");
    assert!(
        options.len() >= 2,
        "configOptions should include mode and model, got: {}",
        response_json
    );
    assert_eq!(options[0]["id"], "mode");
    assert_eq!(options[0]["currentValue"], "dev");
    assert_eq!(options[1]["id"], "model");

    let load_request: LoadSessionRequest = serde_json::from_value(serde_json::json!({
        "sessionId": session_id.to_string(),
        "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
        "mcpServers": []
    }))
    .unwrap();
    let load_response = agent.load_session(load_request).await.unwrap();
    let load_json = serde_json::to_value(&load_response).unwrap();
    assert_eq!(extract_current_mode_id(&load_json), Some("dev".to_string()));
}
