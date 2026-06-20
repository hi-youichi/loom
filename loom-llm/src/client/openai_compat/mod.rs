//! OpenAI-compatible chat completions client using plain `reqwest`, implementing [`crate::traits::LlmClient`].
//!
//! [`ChatOpenAICompat`] speaks the standard `/chat/completions` HTTP + SSE protocol used by OpenAI,
//! Zhipu (BigModel), Kimi, DeepSeek, Ollama, vLLM, LiteLLM, and similar gateways.
//!
//! # Module layout
//!
//! | File | Responsibility |
//! |------|---------------|
//! | `mod.rs` | Struct definition, builder, URL/request helpers |
//! | `request.rs` | Request/response DTOs + message conversion |
//! | `stream.rs` | SSE stream chunk DTOs |
//! | `retry.rs` | Retry constants + error-body parsing |
//! | `audit.rs` | Audit-log helper methods |
//! | `llm_client.rs` | `LlmClient` trait impl (`invoke` / `invoke_stream` / `list_models`) |

mod audit;
mod llm_client;
mod request;
mod retry;
mod stream;

use std::sync::Arc;

use crate::error::LlmError;
use crate::support::audit::LlmAuditLog;
use crate::tool::ToolSpec;
use crate::traits::ToolChoiceMode;

use request::ChatCompletionRequest;
use request::build_request as build_request_dto;

// ---------------------------------------------------------------------------
// Struct + builder
// ---------------------------------------------------------------------------

/// OpenAI-compatible chat completions client (`reqwest`).
///
/// This client uses OpenAI-compatible request and response shapes, including
/// tool calling and SSE streaming. Use the builder-style `with_*` methods to
/// align request behavior with the tool source and prompting strategy used by
/// the surrounding ReAct runtime.
pub struct ChatOpenAICompat {
    pub(super) client: reqwest::Client,
    pub(super) base_url: String,
    pub(super) api_key: String,
    pub(super) model: String,
    pub(super) tools: Option<Vec<ToolSpec>>,
    pub(super) temperature: Option<f32>,
    pub(super) tool_choice: Option<ToolChoiceMode>,
    pub(super) parse_thinking_tags: bool,
    pub(super) headers: Option<crate::traits::LlmHeaders>,
    pub(super) audit_log: Option<Arc<dyn LlmAuditLog>>,
}

impl ChatOpenAICompat {
    /// Builds a client from environment-backed defaults.
    ///
    /// This reads `OPENAI_API_KEY` and optionally `OPENAI_BASE_URL`. The model
    /// name is still provided explicitly so callers can choose it at runtime.
    pub fn new(model: impl Into<String>) -> Result<Self, LlmError> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| LlmError::InvokeFailed("OPENAI_API_KEY is not set".to_string()))?;
        let base_url =
            std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| retry::DEFAULT_BASE_URL.to_string());
        let model = model.into();
        Ok(Self::with_config(base_url, api_key, model))
    }

    /// Builds a client with an explicit base URL, API key, and model.
    pub fn with_config(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            tools: None,
            temperature: None,
            tool_choice: None,
            parse_thinking_tags: true,
            headers: None,
            audit_log: None,
        }
    }

    #[cfg(test)]
    pub fn with_test_client(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            client,
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            tools: None,
            temperature: None,
            tool_choice: None,
            parse_thinking_tags: false,
            headers: None,
            audit_log: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<ToolSpec>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature.clamp(0.0, 1.0));
        self
    }

    pub fn with_tool_choice(mut self, mode: ToolChoiceMode) -> Self {
        self.tool_choice = Some(mode);
        self
    }

    pub fn with_parse_thinking_tags(mut self, enable: bool) -> Self {
        self.parse_thinking_tags = enable;
        self
    }

    pub fn with_audit_log(mut self, audit: Arc<dyn LlmAuditLog>) -> Self {
        self.audit_log = Some(audit);
        self
    }

    pub fn with_headers(mut self, headers: crate::traits::LlmHeaders) -> Self {
        self.headers = Some(headers);
        self
    }

    // -- internal helpers (shared by llm_client.rs + audit.rs) --

    pub(super) fn add_headers_to_request(
        &self,
        request_builder: reqwest::RequestBuilder,
        request_id: &str,
    ) -> reqwest::RequestBuilder {
        let mut builder = request_builder;
        builder = builder.header("X-Request-Id", request_id);
        if let Some(headers) = &self.headers {
            builder = builder.header("X-App-Id", "loom");
            if let Some(thread_id) = &headers.thread_id {
                builder = builder.header("X-Thread-Id", thread_id);
            }
            if let Some(trace_id) = &headers.trace_id {
                builder = builder.header("X-Trace-Id", trace_id);
            }
            for (key, value) in &headers.custom_headers {
                builder = builder.header(key, value);
            }
        }
        builder
    }

    pub(super) fn chat_completions_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/chat/completions", base)
    }

    fn build_request(
        &self,
        messages: &[crate::message::Message],
        stream: bool,
    ) -> ChatCompletionRequest {
        build_request_dto(
            &self.model,
            messages,
            self.tools.as_deref(),
            self.temperature,
            self.tool_choice,
            stream,
        )
    }
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::LlmClient;
    use crate::traits::LlmHeaders;
    use crate::Message;
    use crate::test_util::shared_client::test_client;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use wiremock::matchers::method;

    const CHAT_COMPLETION_RESPONSE: &str = r#"{"id":"chatcmpl-test","object":"chat.completion","created":1234567890,"model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content":"Hello!"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;

    fn extract_x_request_id(req: &wiremock::Request) -> String {
        req.headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_chat_openai_compat_sends_request_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE),
            )
            .mount(&server)
            .await;
        let client = ChatOpenAICompat::with_test_client(
            server.uri(),
            "test-key",
            "gpt-4",
            test_client(),
        );
        let messages = vec![Message::user("Hello!".to_string())];
        let _result = client.invoke(&messages).await.unwrap();
        let received = server.received_requests().await.unwrap();
        assert!(received[0].headers.contains_key("x-request-id"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_chat_openai_compat_generates_unique_request_ids() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE),
            )
            .expect(2)
            .mount(&server)
            .await;
        let client = ChatOpenAICompat::with_test_client(
            server.uri(),
            "test-key",
            "gpt-4",
            test_client(),
        );
        let messages = vec![Message::user("First request".to_string())];
        let _result1 = client.invoke(&messages).await.unwrap();
        let messages = vec![Message::user("Second request".to_string())];
        let _result2 = client.invoke(&messages).await.unwrap();
        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 2);
        let id1 = extract_x_request_id(&received[0]);
        let id2 = extract_x_request_id(&received[1]);
        assert_ne!(id1, id2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_chat_openai_compat_request_id_with_other_headers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE),
            )
            .mount(&server)
            .await;

        let headers = LlmHeaders::default()
            .with_thread_id("test-thread-123")
            .with_trace_id("test-trace-456");

        let client =
            ChatOpenAICompat::with_test_client(server.uri(), "test-key", "gpt-4", test_client())
                .with_headers(headers);
        let messages = vec![Message::user("Hello!".to_string())];
        let _result = client.invoke(&messages).await.unwrap();
        let received = server.received_requests().await.unwrap();
        assert!(received[0].headers.contains_key("x-request-id"));
        assert!(received[0].headers.contains_key("x-app-id"));
        assert!(received[0].headers.contains_key("x-thread-id"));
    }

    const CHAT_COMPLETION_RESPONSE_TRACING: &str = r#"{"id":"chatcmpl-test","object":"chat.completion","created":1,"model":"gpt-4","choices":[{"index":0,"message":{"role":"assistant","content":"mock response"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;

    fn extract_header(req: &wiremock::Request, name: &str) -> Option<String> {
        req.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn root_and_sub_agent_send_same_x_thread_id() {
        let server = MockServer::start().await;
        let shared_trace_id = "shared-trace-id-xyz";

        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(CHAT_COMPLETION_RESPONSE_TRACING),
            )
            .expect(2)
            .mount(&server)
            .await;

        let headers = LlmHeaders::default().with_thread_id(shared_trace_id);

        let root_client =
            ChatOpenAICompat::with_test_client(server.uri(), "test-key", "gpt-4", test_client())
                .with_headers(headers.clone());
        let sub_client =
            ChatOpenAICompat::with_test_client(server.uri(), "test-key", "gpt-4", test_client())
                .with_headers(headers);

        let messages = vec![Message::user("hello")];
        let r1 = root_client.invoke(&messages).await;
        let r2 = sub_client.invoke(&messages).await;

        assert!(r1.is_ok(), "root client request should succeed");
        assert!(r2.is_ok(), "sub-agent client request should succeed");

        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 2);
        let id1 = extract_header(&received[0], "x-thread-id").unwrap_or_default();
        let id2 = extract_header(&received[1], "x-thread-id").unwrap_or_default();
        assert_eq!(id1, id2);
        assert_eq!(id1, shared_trace_id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn trace_thread_id_appears_as_x_thread_id_header() {
        let server = MockServer::start().await;
        let trace_id = "root-trace-from-config";

        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(CHAT_COMPLETION_RESPONSE_TRACING),
            )
            .mount(&server)
            .await;

        let headers = LlmHeaders::default().with_thread_id(trace_id);
        let client =
            ChatOpenAICompat::with_test_client(server.uri(), "test-key", "gpt-4", test_client())
                .with_headers(headers);

        let messages = vec![Message::user("test")];
        let result = client.invoke(&messages).await;
        assert!(result.is_ok());

        let received = server.received_requests().await.unwrap();
        assert_eq!(
            extract_header(&received[0], "x-thread-id"),
            Some(trace_id.to_string())
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn x_thread_id_and_x_app_id_both_present() {
        let server = MockServer::start().await;
        let trace_id = "trace-with-app-id";

        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(CHAT_COMPLETION_RESPONSE_TRACING),
            )
            .mount(&server)
            .await;

        let headers = LlmHeaders::default().with_thread_id(trace_id);
        let client =
            ChatOpenAICompat::with_test_client(server.uri(), "test-key", "gpt-4", test_client())
                .with_headers(headers);

        let messages = vec![Message::user("test")];
        let result = client.invoke(&messages).await;
        assert!(result.is_ok());

        let received = server.received_requests().await.unwrap();
        assert_eq!(
            extract_header(&received[0], "x-thread-id"),
            Some(trace_id.to_string())
        );
        assert_eq!(
            extract_header(&received[0], "x-app-id"),
            Some("loom".to_string())
        );
    }
}
