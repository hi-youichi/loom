use serde_json::{json, Value};
use std::collections::HashMap;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub struct MultiTierMockServer {
    pub server: MockServer,
    response_templates: HashMap<String, String>,
    model_mapping: HashMap<String, String>,
}

impl MultiTierMockServer {
    pub async fn new() -> Self {
        let server = MockServer::start().await;

        let mut response_templates = HashMap::new();
        response_templates.insert(
            "high".to_string(),
            Self::create_tiered_response("high", "High tier response"),
        );
        response_templates.insert(
            "medium".to_string(),
            Self::create_tiered_response("medium", "Medium tier response"),
        );
        response_templates.insert(
            "low".to_string(),
            Self::create_tiered_response("low", "Low tier response"),
        );

        let mut model_mapping = HashMap::new();
        model_mapping.insert("gpt-4-high".to_string(), "high".to_string());
        model_mapping.insert("gpt-3.5-medium".to_string(), "medium".to_string());
        model_mapping.insert("gpt-3.5-low".to_string(), "low".to_string());

        let mock_server = Self {
            server,
            response_templates,
            model_mapping,
        };

        mock_server.setup_endpoints().await;
        mock_server
    }

    fn create_tiered_response(tier: &str, content: &str) -> String {
        json!({
            "model": format!("gpt-{}", tier),
            "choices": [{
                "message": {
                    "content": format!("[TIER:{}] {}", tier, content),
                    "role": "assistant"
                },
                "finish_reason": "stop",
                "index": 0
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30
            }
        })
        .to_string()
    }

    async fn setup_endpoints(&self) {
        // Setup chat completion endpoint
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(self.create_chat_completion_response()),
            )
            .mount(&self.server)
            .await;

        // Setup models endpoint
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(self.create_models_response()))
            .mount(&self.server)
            .await;
    }

    pub fn create_chat_completion_response(&self) -> Value {
        json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4-high",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "[TIER:high] This is a mock response from the high tier model"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30
            }
        })
    }

    fn create_models_response(&self) -> Value {
        json!({
            "object": "list",
            "data": [
                {
                    "id": "gpt-4-high",
                    "object": "model",
                    "created": 1234567890,
                    "owned_by": "test-org"
                },
                {
                    "id": "gpt-3.5-medium",
                    "object": "model",
                    "created": 1234567890,
                    "owned_by": "test-org"
                },
                {
                    "id": "gpt-3.5-low",
                    "object": "model",
                    "created": 1234567890,
                    "owned_by": "test-org"
                }
            ]
        })
    }

    pub fn server_url(&self) -> String {
        self.server.uri()
    }

    pub fn get_tier_from_model(&self, model: &str) -> Option<String> {
        self.model_mapping.get(model).cloned()
    }

    #[allow(dead_code)]
    pub fn set_custom_response(&mut self, tier: &str, content: &str) {
        let response = Self::create_tiered_response(tier, content);
        self.response_templates.insert(tier.to_string(), response);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_multi_tier_mock_server_creation() {
        let server = MultiTierMockServer::new().await;
        assert!(!server.server_url().is_empty());
    }

    #[tokio::test]
    async fn test_tier_response_creation() {
        let response = MultiTierMockServer::create_tiered_response("high", "test content");
        assert!(response.contains("[TIER:high]"));
        assert!(response.contains("test content"));
    }

    #[tokio::test]
    async fn test_model_mapping() {
        let server = MultiTierMockServer::new().await;
        assert_eq!(
            server.get_tier_from_model("gpt-4-high"),
            Some("high".to_string())
        );
        assert_eq!(
            server.get_tier_from_model("gpt-3.5-medium"),
            Some("medium".to_string())
        );
        assert_eq!(
            server.get_tier_from_model("gpt-3.5-low"),
            Some("low".to_string())
        );
    }
}
