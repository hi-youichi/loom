//! Unit tests for ShortTermMemoryToolSource.
//!
//! Verifies: without set_call_context get_recent_messages returns []; with set_call_context
//! returns corresponding messages; with limit returns last N.

mod init_logging;

use graphweave::message::Message;
use graphweave::tool_source::{
    ShortTermMemoryToolSource, ToolCallContext, ToolSource, TOOL_GET_RECENT_MESSAGES,
};
use serde_json::json;

#[tokio::test]
async fn short_term_memory_list_tools_returns_get_recent_messages() {
    let source = ShortTermMemoryToolSource::new().await;
    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, TOOL_GET_RECENT_MESSAGES);
}

#[tokio::test]
async fn short_term_memory_without_context_returns_empty_array() {
    let source = ShortTermMemoryToolSource::new().await;
    let r = source
        .call_tool(TOOL_GET_RECENT_MESSAGES, json!({}))
        .await
        .unwrap();
    assert_eq!(r.text, "[]");
}

#[tokio::test]
async fn short_term_memory_with_context_returns_messages() {
    let source = ShortTermMemoryToolSource::new().await;
    let messages = vec![Message::user("hello"), Message::assistant("hi")];
    source.set_call_context(Some(ToolCallContext::new(messages)));

    let r = source
        .call_tool(TOOL_GET_RECENT_MESSAGES, json!({}))
        .await
        .unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_str(&r.text).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].get("role").and_then(|v| v.as_str()), Some("user"));
    assert_eq!(
        arr[0].get("content").and_then(|v| v.as_str()),
        Some("hello")
    );
    assert_eq!(
        arr[1].get("role").and_then(|v| v.as_str()),
        Some("assistant")
    );
    assert_eq!(arr[1].get("content").and_then(|v| v.as_str()), Some("hi"));
}

#[tokio::test]
async fn short_term_memory_with_limit_returns_last_n() {
    let source = ShortTermMemoryToolSource::new().await;
    let messages = vec![
        Message::user("1"),
        Message::assistant("2"),
        Message::user("3"),
    ];
    source.set_call_context(Some(ToolCallContext::new(messages)));

    let r = source
        .call_tool(TOOL_GET_RECENT_MESSAGES, json!({ "limit": 2 }))
        .await
        .unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_str(&r.text).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].get("content").and_then(|v| v.as_str()), Some("2"));
    assert_eq!(arr[1].get("content").and_then(|v| v.as_str()), Some("3"));
}

/// Verifies call_tool_with_context (explicit context) works without set_call_context (§7.2).
#[tokio::test]
async fn short_term_memory_call_tool_with_context_uses_explicit_ctx() {
    let source = ShortTermMemoryToolSource::new().await;
    let ctx = ToolCallContext::new(vec![Message::user("a"), Message::assistant("b")]);
    let r = source
        .call_tool_with_context(TOOL_GET_RECENT_MESSAGES, json!({}), Some(&ctx))
        .await
        .unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_str(&r.text).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].get("content").and_then(|v| v.as_str()), Some("a"));
    assert_eq!(arr[1].get("content").and_then(|v| v.as_str()), Some("b"));
}
