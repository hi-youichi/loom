use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path};
use serde_json::{json, Value};

pub struct PlanAwareMockServer {
    pub server: MockServer,
}

impl PlanAwareMockServer {
    pub async fn new() -> Self {
        let server = MockServer::start().await;
        let s = Self { server };
        s.setup_endpoints().await;
        s
    }

    async fn setup_endpoints(&self) {
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(Self::simple_response()))
            .mount(&self.server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(Self::models_response()))
            .mount(&self.server)
            .await;
    }

    pub async fn with_plan_response(&self, entries: Vec<Value>) {
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(Self::plan_response(entries)),
            )
            .mount(&self.server)
            .await;
    }

    pub async fn with_multi_step_response(&self) {
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(Self::multi_step_response()),
            )
            .mount(&self.server)
            .await;
    }

    fn simple_response() -> Value {
        json!({
            "id": "chatcmpl-plan-test",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "test-model",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "I'll help you with that. Let me create a plan.\n\n1. Analyze the codebase\n2. Identify issues\n3. Implement fixes"
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

    fn plan_response(entries: Vec<Value>) -> Value {
        json!({
            "id": "chatcmpl-plan-test",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "test-model",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": format!("Here is my plan with {} steps.", entries.len())
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

    fn multi_step_response() -> Value {
        json!({
            "id": "chatcmpl-plan-multi",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "test-model",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "I will execute this in multiple steps:\n\nStep 1: Read and analyze the file\nStep 2: Identify the bug\nStep 3: Apply the fix\nStep 4: Verify the fix works"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 30,
                "total_tokens": 40
            }
        })
    }

    fn models_response() -> Value {
        json!({
            "object": "list",
            "data": [{
                "id": "test-model",
                "object": "model",
                "created": 1234567890,
                "owned_by": "test-org"
            }]
        })
    }

    pub fn server_url(&self) -> String {
        self.server.uri()
    }
}
