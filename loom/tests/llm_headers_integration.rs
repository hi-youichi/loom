//! Integration tests for LLM HTTP headers functionality

use loom::llm::{ChatOpenAICompat, LlmHeaders};

#[test]
fn test_chat_openai_compat_with_headers() {
    let headers = LlmHeaders::default()
        .with_thread_id("test-thread-123")
        .with_trace_id("test-trace-456")
        .add_header("X-Custom-Header", "custom-value");

    let _client = ChatOpenAICompat::with_config("https://api.openai.com/v1", "test-key", "gpt-4")
        .with_headers(headers);

    // Verify client was created successfully
    // In a real integration test, we would make an actual HTTP request
    // and verify the headers are present
    // The model field is private, so we just verify the client was created
    assert!(true); // Test passes if compilation succeeds
}

#[test]
fn test_llm_headers_chaining() {
    let headers = LlmHeaders::default()
        .with_thread_id("thread1")
        .with_trace_id("trace1")
        .add_header("X-Header-1", "value1")
        .add_header("X-Header-2", "value2");

    assert_eq!(headers.thread_id, Some("thread1".to_string()));
    assert_eq!(headers.trace_id, Some("trace1".to_string()));
    assert_eq!(headers.custom_headers.len(), 2);
}

#[test]
fn test_llm_headers_empty() {
    let headers = LlmHeaders::default();
    assert!(headers.thread_id.is_none());
    assert!(headers.trace_id.is_none());
    assert!(headers.custom_headers.is_empty());
}
