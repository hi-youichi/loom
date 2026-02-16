//! Integration tests for TwitterSearchTool: name, spec, argument validation.
//!
//! Does not call the real API; tests only name, spec structure, and input validation.

use graphweave::tools::{Tool, TwitterSearchTool, TOOL_TWITTER_SEARCH};
use serde_json::json;

/// **Scenario**: Tool name returns expected constant.
#[tokio::test]
async fn twitter_search_tool_name_returns_twitter_search() {
    let tool = TwitterSearchTool::new("test_key");
    assert_eq!(tool.name(), TOOL_TWITTER_SEARCH);
}

/// **Scenario**: Spec has correct properties for query, query_type, cursor.
#[tokio::test]
async fn twitter_search_tool_spec_has_correct_properties() {
    let tool = TwitterSearchTool::new("test_key");
    let spec = tool.spec();
    assert_eq!(spec.name, TOOL_TWITTER_SEARCH);
    assert!(spec.description.is_some());
    let desc = spec.description.unwrap();
    assert!(desc.contains("Search") && desc.contains("query"));
    assert_eq!(spec.input_schema["properties"]["query"]["type"], "string");
    assert_eq!(
        spec.input_schema["properties"]["query_type"]["enum"],
        json!(["Latest", "Top"])
    );
    assert!(spec.input_schema["required"]
        .as_array()
        .unwrap()
        .contains(&json!("query")));
}

/// **Scenario**: Missing query returns InvalidInput error.
#[tokio::test]
async fn twitter_search_tool_call_missing_query_returns_error() {
    let tool = TwitterSearchTool::new("test_key");
    let args = json!({});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("query") || err.to_string().contains("InvalidInput"));
}

/// **Scenario**: Empty query returns InvalidInput error.
#[tokio::test]
async fn twitter_search_tool_call_empty_query_returns_error() {
    let tool = TwitterSearchTool::new("test_key");
    let args = json!({"query": "   "});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("empty") || err.to_string().contains("InvalidInput"));
}

/// **Scenario**: Invalid query_type returns InvalidInput error.
#[tokio::test]
async fn twitter_search_tool_call_invalid_query_type_returns_error() {
    let tool = TwitterSearchTool::new("test_key");
    let args = json!({"query": "AI", "query_type": "Invalid"});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Latest") || err.to_string().contains("InvalidInput"));
}
