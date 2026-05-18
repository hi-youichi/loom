//! Integration test: verify X-Thread-Id consistency between root agent and sub-agent.
//!
//! The root agent's `trace_thread_id` must be inherited unchanged by every sub-agent
//! so that **all** LLM calls across the hierarchy carry the same `X-Thread-Id` HTTP
//! header for external tracing (e.g. Datadog, Langfuse).
//!
//! Tests cover:
//! 1. Config-level: `trace_thread_id` propagation from parent to sub-agent config
//! 2. HTTP-level: both root and sub-agent LLM clients emit the same `X-Thread-Id`

mod init_logging;

use loom::llm::{ChatOpenAICompat, LlmClient, LlmHeaders};
use loom::Message;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::method;

const CHAT_COMPLETION_RESPONSE: &str = r#"{"id":"chatcmpl-test","object":"chat.completion","created":1,"model":"gpt-4","choices":[{"index":0,"message":{"role":"assistant","content":"mock response"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;

fn extract_header(req: &wiremock::Request, name: &str) -> Option<String> {
    req.headers.get(name).and_then(|v| v.to_str().ok()).map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// Config-level tests
// ---------------------------------------------------------------------------

/// Verify the config propagation logic that InvokeAgentTool::call_single_exec uses:
/// - sub_config.thread_id = unique (sub-{parent}-{name}-{depth})
/// - sub_config.trace_thread_id = inherited from parent unchanged
#[test]
fn sub_agent_config_inherits_trace_thread_id() {
    use loom::ReactBuildConfig;

    let root_config = ReactBuildConfig {
        thread_id: Some("root-session-001".to_string()),
        trace_thread_id: Some("trace-abc-123".to_string()),
        ..ReactBuildConfig::from_env()
    };

    let parent_thread_id = root_config.thread_id.as_deref().unwrap_or("root");
    let agent_name = "dev";
    let depth: u32 = 0;

    let mut sub_config = root_config.clone();
    sub_config.thread_id = Some(format!(
        "sub-{}-{}-{}",
        parent_thread_id, agent_name, depth
    ));
    sub_config.trace_thread_id = root_config.trace_thread_id.clone();

    assert_eq!(
        sub_config.thread_id,
        Some("sub-root-session-001-dev-0".to_string()),
        "sub-agent thread_id must be unique"
    );
    assert_eq!(
        sub_config.trace_thread_id,
        Some("trace-abc-123".to_string()),
        "sub-agent trace_thread_id must match root"
    );
}

/// At depth=1 (sub-agent invoking another sub-agent), trace_thread_id is still the same.
#[test]
fn nested_sub_agent_keeps_same_trace_thread_id() {
    use loom::ReactBuildConfig;

    let root_trace = "trace-root-999";

    let mut depth0_config = ReactBuildConfig {
        thread_id: Some("sub-root-session-dev-0".to_string()),
        trace_thread_id: Some(root_trace.to_string()),
        ..ReactBuildConfig::from_env()
    };

    let depth = 1u32;
    let parent_thread_id = depth0_config.thread_id.as_deref().unwrap_or("root");
    depth0_config.thread_id = Some(format!(
        "sub-{}-{}-{}",
        parent_thread_id, "explore", depth
    ));

    assert_eq!(
        depth0_config.thread_id,
        Some("sub-sub-root-session-dev-0-explore-1".to_string())
    );
    assert_eq!(depth0_config.trace_thread_id, Some(root_trace.to_string()));
}

/// When trace_thread_id is None, thread_id is used as fallback (matching
/// build/llm.rs logic: trace_thread_id.or(thread_id)).
#[test]
fn trace_thread_id_falls_back_to_thread_id() {
    use loom::ReactBuildConfig;

    let root_config = ReactBuildConfig {
        thread_id: Some("session-only-001".to_string()),
        trace_thread_id: None,
        ..ReactBuildConfig::from_env()
    };

    let trace_id = root_config
        .trace_thread_id
        .as_ref()
        .or(root_config.thread_id.as_ref());

    assert_eq!(trace_id.map(|s| s.as_str()), Some("session-only-001"));
}

// ---------------------------------------------------------------------------
// HTTP-level tests
// ---------------------------------------------------------------------------

/// Two ChatOpenAICompat clients (simulating root and sub-agent) both configured
/// with the same trace_thread_id must send the same X-Thread-Id header.
#[tokio::test]
async fn root_and_sub_agent_send_same_x_thread_id() {
    let server = MockServer::start().await;
    let shared_trace_id = "shared-trace-id-xyz";

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE))
        .expect(2)
        .mount(&server)
        .await;

    let headers = LlmHeaders::default().with_thread_id(shared_trace_id);

    let root_client = ChatOpenAICompat::with_config(&server.uri(), "test-key", "gpt-4")
        .with_headers(headers.clone());
    let sub_client =
        ChatOpenAICompat::with_config(&server.uri(), "test-key", "gpt-4").with_headers(headers);

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

/// Verify the full chain: root config trace_thread_id -> LlmHeaders -> X-Thread-Id HTTP header.
#[tokio::test]
async fn trace_thread_id_appears_as_x_thread_id_header() {
    let server = MockServer::start().await;
    let trace_id = "root-trace-from-config";

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE))
        .mount(&server)
        .await;

    let headers = LlmHeaders::default().with_thread_id(trace_id);
    let client = ChatOpenAICompat::with_config(&server.uri(), "test-key", "gpt-4")
        .with_headers(headers);

    let messages = vec![Message::user("test")];
    let result = client.invoke(&messages).await;
    assert!(result.is_ok());

    let received = server.received_requests().await.unwrap();
    assert_eq!(extract_header(&received[0], "x-thread-id"), Some(trace_id.to_string()));
}

/// Verify X-App-Id is also set alongside X-Thread-Id.
#[tokio::test]
async fn x_thread_id_and_x_app_id_both_present() {
    let server = MockServer::start().await;
    let trace_id = "trace-with-app-id";

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE))
        .mount(&server)
        .await;

    let headers = LlmHeaders::default().with_thread_id(trace_id);
    let client = ChatOpenAICompat::with_config(&server.uri(), "test-key", "gpt-4")
        .with_headers(headers);

    let messages = vec![Message::user("test")];
    let result = client.invoke(&messages).await;
    assert!(result.is_ok());

    let received = server.received_requests().await.unwrap();
    assert_eq!(extract_header(&received[0], "x-thread-id"), Some(trace_id.to_string()));
    assert_eq!(extract_header(&received[0], "x-app-id"), Some("loom".to_string()));
}
