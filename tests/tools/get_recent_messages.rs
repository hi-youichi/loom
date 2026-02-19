use loom::tools::{GetRecentMessagesTool, TOOL_GET_RECENT_MESSAGES};
use loom::message::Message;
use loom::tool_source::ToolCallContext;
use serde_json::json;

#[tokio::test]
async fn get_recent_messages_tool_name_returns_get_recent_messages() {
    let tool = GetRecentMessagesTool::new();
    assert_eq!(tool.name(), TOOL_GET_RECENT_MESSAGES);
}

#[tokio::test]
async fn get_recent_messages_tool_spec_has_correct_properties() {
    let tool = GetRecentMessagesTool::new();
    let spec = tool.spec();
    assert_eq!(spec.name, TOOL_GET_RECENT_MESSAGES);
    assert!(spec.description.is_some());
    assert!(spec.description.unwrap().contains("recent messages"));
    assert_eq!(spec.input_schema["properties"]["limit"]["type"], "integer");
}

#[tokio::test]
async fn get_recent_messages_tool_call_without_context() {
    let tool = GetRecentMessagesTool::new();
    let args = json!({});
    let result = tool.call(args, None).await.unwrap();
    let messages: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(messages.len(), 0);
}

#[tokio::test]
async fn get_recent_messages_tool_call_with_context() {
    let tool = GetRecentMessagesTool::new();
    let context = ToolCallContext::new(vec![
        Message::User("hello".to_string()),
        Message::Assistant("hi there!".to_string()),
        Message::User("how are you?".to_string()),
    ]);

    let args = json!({});
    let result = tool.call(args, Some(&context)).await.unwrap();
    let messages: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "hello");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "hi there!");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"], "how are you?");
}

#[tokio::test]
async fn get_recent_messages_tool_call_with_limit() {
    let tool = GetRecentMessagesTool::new();
    let context = ToolCallContext::new(vec![
        Message::User("msg1".to_string()),
        Message::Assistant("msg2".to_string()),
        Message::User("msg3".to_string()),
        Message::Assistant("msg4".to_string()),
        Message::User("msg5".to_string()),
    ]);

    let args = json!({"limit": 2});
    let result = tool.call(args, Some(&context)).await.unwrap();
    let messages: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["content"], "msg4");
    assert_eq!(messages[1]["content"], "msg5");
}

#[tokio::test]
async fn get_recent_messages_tool_call_limit_exceeds_messages() {
    let tool = GetRecentMessagesTool::new();
    let context = ToolCallContext::new(vec![
        Message::User("msg1".to_string()),
        Message::Assistant("msg2".to_string()),
    ]);

    let args = json!({"limit": 10});
    let result = tool.call(args, Some(&context)).await.unwrap();
    let messages: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn get_recent_messages_tool_call_limit_zero() {
    let tool = GetRecentMessagesTool::new();
    let context = ToolCallContext::new(vec![
        Message::User("msg1".to_string()),
        Message::Assistant("msg2".to_string()),
    ]);

    let args = json!({"limit": 0});
    let result = tool.call(args, Some(&context)).await.unwrap();
    let messages: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(messages.len(), 0);
}

#[tokio::test]
async fn get_recent_messages_tool_includes_system_messages() {
    let tool = GetRecentMessagesTool::new();
    let context = ToolCallContext::new(vec![
        Message::System("You are a helpful assistant.".to_string()),
        Message::User("Hello".to_string()),
        Message::Assistant("Hi!".to_string()),
    ]);

    let args = json!({});
    let result = tool.call(args, Some(&context)).await.unwrap();
    let messages: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[2]["role"], "assistant");
}

#[tokio::test]
async fn get_recent_messages_tool_ignores_extra_args() {
    let tool = GetRecentMessagesTool::new();
    let context = ToolCallContext::new(vec![Message::User("hello".to_string())]);

    let args = json!({"limit": 1, "unused": "param"});
    let result = tool.call(args, Some(&context)).await.unwrap();
    let messages: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(messages.len(), 1);
}

#[tokio::test]
async fn get_recent_messages_tool_handles_empty_context() {
    let tool = GetRecentMessagesTool::new();
    let context = ToolCallContext::new(vec![]);

    let args = json!({});
    let result = tool.call(args, Some(&context)).await.unwrap();
    let messages: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(messages.len(), 0);
}
