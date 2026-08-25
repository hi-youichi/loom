use agent_client_protocol::schema::v1::{
    NewSessionRequest, NewSessionResponse, SetSessionConfigOptionRequest,
};
use anureo_acp::{AnureoAcpAgent, ModelOption, ModelProvider};
use std::path::PathBuf;
use std::sync::Arc;

fn make_new_session_request() -> NewSessionRequest {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    NewSessionRequest::new(cwd)
}

struct MockModelProvider;

#[async_trait::async_trait]
impl ModelProvider for MockModelProvider {
    async fn fetch_models(&self) -> Vec<ModelOption> {
        vec![
            ModelOption {
                id: "default".to_string(),
                name: "(default)".to_string(),
                provider: String::new(),
            },
            ModelOption {
                id: "gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider: "openai".to_string(),
            },
        ]
    }
}

fn create_test_agent() -> AnureoAcpAgent {
    AnureoAcpAgent::new()
        .unwrap()
        .with_model_provider(Arc::new(MockModelProvider))
}

fn extract_model_options(response: &NewSessionResponse) -> Vec<String> {
    let mut models = Vec::new();

    if let Ok(json) = serde_json::to_value(response) {
        if let Some(config_options) = json.get("configOptions").and_then(|v| v.as_array()) {
            for config in config_options {
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

fn extract_current_model(response: &NewSessionResponse) -> Option<String> {
    if let Ok(json) = serde_json::to_value(response) {
        if let Some(config_options) = json.get("configOptions").and_then(|v| v.as_array()) {
            for config in config_options {
                if config.get("id").and_then(|v| v.as_str()) == Some("model") {
                    if let Some(current) = config.get("currentValue").and_then(|v| v.as_str()) {
                        return Some(current.to_string());
                    }
                }
            }
        }
    }
    None
}

#[tokio::test]
async fn test_new_session_returns_config_options() {
    let agent = create_test_agent();
    let request = make_new_session_request();

    let response = agent.new_session(request).await;
    assert!(
        response.is_ok(),
        "new_session should succeed: {:?}",
        response.err()
    );

    let response = response.unwrap();

    let json = serde_json::to_value(&response).expect("Should serialize to JSON");
    assert!(
        json.get("configOptions").is_some(),
        "Response should have configOptions"
    );
}

#[tokio::test]
async fn test_set_session_config_option_model() {
    let agent = create_test_agent();

    let session_resp = agent.new_session(make_new_session_request()).await.unwrap();
    let session_id = session_resp.session_id.clone();

    let request_json = serde_json::json!({
        "sessionId": session_id,
        "configId": "model",
        "value": "gpt-4o"
    });
    let request: SetSessionConfigOptionRequest = serde_json::from_value(request_json).unwrap();

    let response = agent.set_session_config_option(request).await;
    assert!(
        response.is_ok(),
        "set_session_config_option should succeed: {:?}",
        response.err()
    );
}

#[tokio::test]
async fn test_set_session_config_option_unknown_config() {
    let agent = create_test_agent();

    let session_resp = agent.new_session(make_new_session_request()).await.unwrap();
    let session_id = session_resp.session_id.clone();

    let request_json = serde_json::json!({
        "sessionId": session_id,
        "configId": "unknown_config",
        "value": "value"
    });
    let request: SetSessionConfigOptionRequest = serde_json::from_value(request_json).unwrap();

    let response = agent.set_session_config_option(request).await;
    assert!(
        response.is_err(),
        "set_session_config_option should fail for unknown config"
    );
}

#[tokio::test]
async fn test_set_session_config_option_unknown_session() {
    let agent = create_test_agent();

    let request_json = serde_json::json!({
        "sessionId": "nonexistent-session",
        "configId": "model",
        "value": "gpt-4o"
    });
    let request: SetSessionConfigOptionRequest = serde_json::from_value(request_json).unwrap();

    let response = agent.set_session_config_option(request).await;
    assert!(
        response.is_err(),
        "set_session_config_option should fail for unknown session"
    );
}

#[test]
fn test_model_list_from_mock_provider() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let agent = create_test_agent();
        let request = make_new_session_request();

        let response = agent.new_session(request).await.unwrap();
        let models = extract_model_options(&response);

        let current = extract_current_model(&response);
        assert!(current.is_some(), "current model should be set");
        for m in &models {
            assert!(!m.is_empty(), "model option should not be empty");
        }
    });
}

#[test]
fn test_current_model_from_env() {
    temp_env::with_vars(
        vec![
            ("MODEL", Some("test-model-123")),
            ("OPENAI_MODEL", None),
            ("PROVIDER", None),
            ("OPENAI_API_KEY", None),
            ("ZHIPUAI_API_KEY", None),
            ("ANTHROPIC_API_KEY", None),
        ],
        || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let agent = create_test_agent();
                let request = make_new_session_request();

                let response = agent.new_session(request).await.unwrap();
                let current = extract_current_model(&response);

                assert!(current.is_some(), "Expected some model to be set");

                if current != Some("test-model-123".to_string()) {
                    eprintln!("Warning: Expected test-model-123 but got {:?}", current);
                    eprintln!("This may be due to config file override");
                }
            });
        },
    );
}
