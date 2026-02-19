//! Unit tests for MockLLM and LlmClient.
//!
//! Verifies MockLlm returns expected content and tool_calls; with_no_tool_calls
//! returns empty tool_calls for END path.

mod init_logging;

use loom::{LlmClient, Message, MockLlm};

#[tokio::test]
async fn mock_llm_with_get_time_returns_content_and_one_tool_call() {
    let llm = MockLlm::with_get_time_call();
    let messages = vec![Message::user("What time is it?")];
    let out = llm.invoke(&messages).await.unwrap();
    assert_eq!(out.content, "I'll check the time.");
    assert_eq!(out.tool_calls.len(), 1);
    assert_eq!(out.tool_calls[0].name, "get_time");
    assert_eq!(out.tool_calls[0].arguments, "{}");
    assert_eq!(out.tool_calls[0].id.as_deref(), Some("call-1"));
}

#[tokio::test]
async fn mock_llm_with_no_tool_calls_returns_content_and_empty_tool_calls() {
    let llm = MockLlm::with_no_tool_calls("Hello, no tools.");
    let messages = vec![Message::user("Hi")];
    let out = llm.invoke(&messages).await.unwrap();
    assert_eq!(out.content, "Hello, no tools.");
    assert!(out.tool_calls.is_empty());
}

#[tokio::test]
async fn mock_llm_new_custom_content_and_tool_calls() {
    let llm = MockLlm::new(
        "Custom reply",
        vec![loom::ToolCall {
            name: "search".into(),
            arguments: r#"{"q":"x"}"#.into(),
            id: Some("id-1".into()),
        }],
    );
    let messages = vec![Message::user("Search for x")];
    let out = llm.invoke(&messages).await.unwrap();
    assert_eq!(out.content, "Custom reply");
    assert_eq!(out.tool_calls.len(), 1);
    assert_eq!(out.tool_calls[0].name, "search");
    assert_eq!(out.tool_calls[0].arguments, r#"{"q":"x"}"#);
}

#[tokio::test]
async fn mock_llm_ignores_input_messages_returns_fixed_response() {
    let llm = MockLlm::with_get_time_call();
    let empty: Vec<Message> = vec![];
    let out = llm.invoke(&empty).await.unwrap();
    assert_eq!(out.content, "I'll check the time.");
    assert_eq!(out.tool_calls[0].name, "get_time");
}
