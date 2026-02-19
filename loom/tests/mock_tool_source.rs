//! Unit tests for Mock ToolSource.
//!
//! Verifies list_tools() returns fixed tool list, call_tool() returns fixed text;
//! Act can call call_tool and get result (no MCP Server).

mod init_logging;

use loom::{MockToolSource, ToolSource};
use serde_json::json;

#[tokio::test]
async fn mock_tool_source_list_tools_returns_get_time_example() {
    let source = MockToolSource::get_time_example();
    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "get_time");
    assert!(tools[0]
        .description
        .as_deref()
        .unwrap_or("")
        .contains("Get current time"));
}

#[tokio::test]
async fn mock_tool_source_call_tool_returns_fixed_text() {
    let source = MockToolSource::get_time_example();
    let result = source.call_tool("get_time", json!({})).await.unwrap();
    assert_eq!(result.text, "2025-01-29 12:00:00");
}

#[tokio::test]
async fn mock_tool_source_call_tool_any_name_returns_same_result() {
    let source = MockToolSource::get_time_example();
    let r1 = source.call_tool("get_time", json!({})).await.unwrap();
    let r2 = source
        .call_tool("other_tool", json!({"x":1}))
        .await
        .unwrap();
    assert_eq!(r1.text, r2.text);
    assert_eq!(r1.text, "2025-01-29 12:00:00");
}

#[tokio::test]
async fn mock_tool_source_custom_call_result() {
    let source = MockToolSource::get_time_example().with_call_result("custom result".to_string());
    let result = source.call_tool("get_time", json!({})).await.unwrap();
    assert_eq!(result.text, "custom result");
}

#[tokio::test]
async fn mock_tool_source_new_custom_tools_and_result() {
    let source = MockToolSource::new(
        vec![loom::ToolSpec {
            name: "search".to_string(),
            description: Some("Search.".to_string()),
            input_schema: json!({ "type": "object", "properties": { "q": {} } }),
        }],
        "[]".to_string(),
    );
    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "search");
    let result = source
        .call_tool("search", json!({"q":"rust"}))
        .await
        .unwrap();
    assert_eq!(result.text, "[]");
}
