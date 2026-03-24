//! ACP Agent integration tests for model selection feature.
//!
//! These tests verify the ACP model selection functionality using the real
//! provider configuration from ~/.loom/config.toml.

use loom_acp::LoomAcpAgent;
use agent_client_protocol::{
    Agent, NewSessionRequest, NewSessionResponse, SetSessionConfigOptionRequest,
};
use std::path::PathBuf;

/// Helper to create a NewSessionRequest with a temp directory.
fn make_new_session_request() -> NewSessionRequest {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    NewSessionRequest::new(cwd)
}

/// Helper to extract model options from NewSessionResponse.
fn extract_model_options(response: &NewSessionResponse) -> Vec<String> {
    let mut models = Vec::new();
    
    // Serialize response to JSON to inspect structure
    if let Ok(json) = serde_json::to_value(response) {
        if let Some(config_options) = json.get("configOptions").and_then(|v| v.as_array()) {
            for config in config_options {
                // Each config should have an "options" array
                if let Some(options) = config.get("options").and_then(|v| v.as_array()) {
                    for option in options {
                        if let Some(id) = option.get("value").and_then(|v| v.as_str()) {
                            models.push(id.to_string());
                        }
                    }
                }
            }
        }
    }
    
    models
}

/// Helper to extract current model from NewSessionResponse.
fn extract_current_model(response: &NewSessionResponse) -> Option<String> {
    if let Ok(json) = serde_json::to_value(response) {
        if let Some(config_options) = json.get("configOptions").and_then(|v| v.as_array()) {
            for config in config_options {
                if let Some(current) = config.get("currentValue").and_then(|v| v.as_str()) {
                    return Some(current.to_string());
                }
            }
        }
    }
    None
}

/// Test that new_session returns a response with config_options.
#[tokio::test]
async fn test_new_session_returns_config_options() {
    let agent = LoomAcpAgent::new();
    let request = make_new_session_request();
    
    let response = agent.new_session(request).await;
    assert!(response.is_ok(), "new_session should succeed: {:?}", response.err());
    
    let response = response.unwrap();
    
    // Serialize to JSON to check structure
    let json = serde_json::to_value(&response).expect("Should serialize to JSON");
    assert!(json.get("configOptions").is_some(), "Response should have configOptions");
}

/// Test that set_session_config_option works for model config.
#[tokio::test]
async fn test_set_session_config_option_model() {
    let agent = LoomAcpAgent::new();
    
    // First create a session
    let session_resp = agent.new_session(make_new_session_request()).await.unwrap();
    let session_id = session_resp.session_id.clone();
    
    // Now try to set the model - construct request via JSON since types are non_exhaustive
    let request_json = serde_json::json!({
        "sessionId": session_id,
        "configId": "model",
        "value": "gpt-4o"
    });
    let request: SetSessionConfigOptionRequest = serde_json::from_value(request_json).unwrap();
    
    let response = agent.set_session_config_option(request).await;
    assert!(response.is_ok(), "set_session_config_option should succeed: {:?}", response.err());
}

/// Test that set_session_config_option rejects unknown config_id.
#[tokio::test]
async fn test_set_session_config_option_unknown_config() {
    let agent = LoomAcpAgent::new();
    
    // First create a session
    let session_resp = agent.new_session(make_new_session_request()).await.unwrap();
    let session_id = session_resp.session_id.clone();
    
    // Try to set an unknown config
    let request_json = serde_json::json!({
        "sessionId": session_id,
        "configId": "unknown_config",
        "value": "value"
    });
    let request: SetSessionConfigOptionRequest = serde_json::from_value(request_json).unwrap();
    
    let response = agent.set_session_config_option(request).await;
    assert!(response.is_err(), "set_session_config_option should fail for unknown config");
}

/// Test that set_session_config_option fails for unknown session.
#[tokio::test]
async fn test_set_session_config_option_unknown_session() {
    let agent = LoomAcpAgent::new();
    
    let request_json = serde_json::json!({
        "sessionId": "nonexistent-session",
        "configId": "model",
        "value": "gpt-4o"
    });
    let request: SetSessionConfigOptionRequest = serde_json::from_value(request_json).unwrap();
    
    let response = agent.set_session_config_option(request).await;
    assert!(response.is_err(), "set_session_config_option should fail for unknown session");
}

/// Test model list from real provider.
/// This test requires a valid provider configuration and API key.
/// Run with: cargo test -p loom-acp --test agent_integration -- --ignored model_list
#[tokio::test]
#[ignore = "Requires real API key and provider configuration"]
async fn test_model_list_from_real_provider() {
    let agent = LoomAcpAgent::new();
    let request = make_new_session_request();
    
    let response = agent.new_session(request).await.unwrap();
    
    let models = extract_model_options(&response);
    
    // If provider is configured and accessible, we should get some models
    if !models.is_empty() {
        println!("Found {} models:", models.len());
        for model in &models {
            println!("  - {}", model);
        }
    } else {
        println!("No models found - check provider configuration");
    }
    
    // The test passes either way - it's informational
}

/// Test that current model is set from environment or default.
#[tokio::test]
async fn test_current_model_from_env() {
    // Save current env
    let original_model = std::env::var("MODEL").ok();
    let original_openai_model = std::env::var("OPENAI_MODEL").ok();
    
    // Set a test model
    unsafe {
        std::env::set_var("MODEL", "test-model-123");
    }
    
    let agent = LoomAcpAgent::new();
    let request = make_new_session_request();
    
    let response = agent.new_session(request).await.unwrap();
    let current = extract_current_model(&response);
    
    assert_eq!(current, Some("test-model-123".to_string()));
    
    // Restore env
    unsafe {
        if let Some(orig) = original_model {
            std::env::set_var("MODEL", orig);
        } else {
            std::env::remove_var("MODEL");
        }
        if let Some(orig) = original_openai_model {
            std::env::set_var("OPENAI_MODEL", orig);
        }
    }
}
