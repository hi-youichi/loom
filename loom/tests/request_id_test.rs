use loom::llm::{ChatOpenAICompat, LlmClient, LlmHeaders};
use loom::Message;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::method;

const CHAT_COMPLETION_RESPONSE: &str = r#"{"id":"chatcmpl-test","object":"chat.completion","created":1234567890,"model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content":"Hello!"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;

fn extract_x_request_id(req: &wiremock::Request) -> String {
    req.headers.get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

#[tokio::test]
async fn test_chat_openai_compat_sends_request_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE))
        .mount(&server)
        .await;
    let client = ChatOpenAICompat::with_config(&server.uri(), "test-key", "gpt-4");
    let messages = vec![Message::user("Hello!".to_string())];
    let _result = client.invoke(&messages).await.unwrap();
    let received = server.received_requests().await.unwrap();
    assert!(received[0].headers.contains_key("x-request-id"));
}

#[tokio::test]
async fn test_chat_openai_compat_generates_unique_request_ids() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE))
        .expect(2)
        .mount(&server)
        .await;
    let client = ChatOpenAICompat::with_config(&server.uri(), "test-key", "gpt-4");
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

#[tokio::test]
async fn test_chat_openai_compat_request_id_with_other_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE))
        .mount(&server)
        .await;

    let headers = LlmHeaders::default()
        .with_thread_id("test-thread-123")
        .with_trace_id("test-trace-456");

    let client = ChatOpenAICompat::with_config(&server.uri(), "test-key", "gpt-4").with_headers(headers);
    let messages = vec![Message::user("Hello!".to_string())];
    let _result = client.invoke(&messages).await.unwrap();
    let received = server.received_requests().await.unwrap();
    assert!(received[0].headers.contains_key("x-request-id"));
    assert!(received[0].headers.contains_key("x-app-id"));
    assert!(received[0].headers.contains_key("x-thread-id"));
}