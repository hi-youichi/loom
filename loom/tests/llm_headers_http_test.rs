use loom::llm::{ChatOpenAICompat, LlmClient, LlmHeaders};
use loom::Message;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::method;

const CHAT_COMPLETION_RESPONSE: &str = r#"{"id":"chatcmpl-test","object":"chat.completion","created":1,"model":"gpt-4","choices":[{"index":0,"message":{"role":"assistant","content":"test response"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;

#[tokio::test]
async fn chat_openai_compat_sends_x_thread_id() {
    let server = MockServer::start().await;
    let expected_thread_id = "test-thread-12345";
    let expected_trace_id = "test-trace-67890";

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE))
        .mount(&server)
        .await;

    let headers = LlmHeaders::default()
        .with_thread_id(expected_thread_id)
        .with_trace_id(expected_trace_id);

    let client = ChatOpenAICompat::with_config(&server.uri(), "test-key", "gpt-4").with_headers(headers);
    let messages = vec![Message::user("test message")];
    let response = client.invoke(&messages).await;

    assert!(response.is_ok());

    let received = server.received_requests().await.unwrap();
    assert!(received.len() >= 1);
    let req = &received[0];
    assert!(req.headers.contains_key("x-thread-id"));
    assert!(req.headers.contains_key("x-trace-id"));
    assert!(req.headers.contains_key("x-app-id"));
}

#[tokio::test]
async fn chat_openai_compat_sends_custom_headers() {
    let server = MockServer::start().await;
    let expected_custom_value = "custom-value-123";

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE))
        .mount(&server)
        .await;

    let headers = LlmHeaders::default()
        .add_header("X-Custom-Header", expected_custom_value);

    let client = ChatOpenAICompat::with_config(&server.uri(), "test-key", "gpt-4").with_headers(headers);
    let messages = vec![Message::user("test")];
    let response = client.invoke(&messages).await;

    assert!(response.is_ok());

    let received = server.received_requests().await.unwrap();
    assert!(received.len() >= 1);
    let req = &received[0];
    assert!(req.headers.contains_key("x-custom-header"));
    assert!(req.headers.contains_key("x-app-id"));
}

#[tokio::test]
async fn chat_openai_compat_sends_all_headers_combined() {
    let server = MockServer::start().await;
    let expected_thread_id = "combined-thread-123";
    let expected_trace_id = "combined-trace-456";
    let expected_custom_1 = "value-1";
    let expected_custom_2 = "value-2";

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE))
        .mount(&server)
        .await;

    let headers = LlmHeaders::default()
        .with_thread_id(expected_thread_id)
        .with_trace_id(expected_trace_id)
        .add_header("X-Custom-1", expected_custom_1)
        .add_header("X-Custom-2", expected_custom_2);

    let client = ChatOpenAICompat::with_config(&server.uri(), "test-key", "gpt-4").with_headers(headers);
    let messages = vec![Message::user("test")];
    let response = client.invoke(&messages).await;

    assert!(response.is_ok());

    let received = server.received_requests().await.unwrap();
    assert!(received.len() >= 1);
    let req = &received[0];
    assert!(req.headers.contains_key("x-thread-id"));
    assert!(req.headers.contains_key("x-trace-id"));
    assert!(req.headers.contains_key("x-app-id"));
    assert!(req.headers.contains_key("x-custom-1"));
    assert!(req.headers.contains_key("x-custom-2"));
}

#[tokio::test]
async fn chat_openai_compat_stream_request_sends_headers() {
    let server = MockServer::start().await;
    let expected_thread_id = "stream-thread-789";

    let sse_body = r#"data: {"id":"chatcmpl-test","object":"chat.completion.chunk","created":1,"model":"gpt-4","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}
data: {"id":"chatcmpl-test","object":"chat.completion.chunk","created":1,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}
data: {"id":"chatcmpl-test","object":"chat.completion.chunk","created":1,"model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}
data: [DONE]
"#;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(sse_body))
        .mount(&server)
        .await;

    let headers = LlmHeaders::default()
        .with_thread_id(expected_thread_id);

    let client = ChatOpenAICompat::with_config(&server.uri(), "test-key", "gpt-4").with_headers(headers);
    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel(10);
    let messages = vec![Message::user("test")];

    let handle = tokio::spawn(async move {
        client.invoke_stream(&messages, Some(chunk_tx)).await
    });

    let mut received_chunks = 0;
    while let Some(_chunk) = chunk_rx.recv().await {
        received_chunks += 1;
    }

    assert!(received_chunks > 0);

    let response = handle.await.unwrap();
    assert!(response.is_ok());

    let received = server.received_requests().await.unwrap();
    assert!(received.len() >= 1);
    let req = &received[0];
    assert!(req.headers.contains_key("x-thread-id"));
    assert!(req.headers.contains_key("x-app-id"));
}

#[tokio::test]
async fn chat_openai_compat_without_headers_no_custom_headers_sent() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CHAT_COMPLETION_RESPONSE))
        .mount(&server)
        .await;

    let client = ChatOpenAICompat::with_config(&server.uri(), "test-key", "gpt-4");
    let messages = vec![Message::user("test")];
    let response = client.invoke(&messages).await;

    assert!(response.is_ok());

    let received = server.received_requests().await.unwrap();
    assert!(received.len() >= 1);
    let req = &received[0];
    assert!(!req.headers.contains_key("x-thread-id"));
    assert!(!req.headers.contains_key("x-trace-id"));
    assert!(!req.headers.contains_key("x-custom-"));
    assert!(!req.headers.contains_key("x-app-id"));
}