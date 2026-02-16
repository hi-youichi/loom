//! Integration tests for WebFetcherTool: name, spec, GET/POST call behavior.

use graphweave::tools::{Tool, WebFetcherTool, TOOL_WEB_FETCHER};
use serde_json::json;

#[tokio::test]
async fn web_fetcher_tool_name_returns_web_fetcher() {
    let tool = WebFetcherTool::new();
    assert_eq!(tool.name(), TOOL_WEB_FETCHER);
}

#[tokio::test]
async fn web_fetcher_tool_spec_has_correct_properties() {
    let tool = WebFetcherTool::new();
    let spec = tool.spec();
    assert_eq!(spec.name, TOOL_WEB_FETCHER);
    assert!(spec.description.is_some());
    let desc = spec.description.unwrap();
    assert!(desc.contains("URL") && (desc.contains("GET") || desc.contains("POST")));
    assert_eq!(spec.input_schema["properties"]["url"]["type"], "string");
    assert!(spec.input_schema["required"]
        .as_array()
        .unwrap()
        .contains(&json!("url")));
    assert_eq!(
        spec.input_schema["properties"]["method"]["enum"],
        json!(["GET", "POST"])
    );
}

#[tokio::test]
async fn web_fetcher_tool_call_fetches_valid_url() {
    let tool = WebFetcherTool::new();
    let args = json!({"url": "https://httpbin.org/json"});
    let result = tool.call(args, None).await.unwrap();
    assert!(result.text.contains("slideshow"));
}

#[tokio::test]
async fn web_fetcher_tool_call_missing_url_returns_error() {
    let tool = WebFetcherTool::new();
    let args = json!({});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("missing") || err.to_string().contains("InvalidInput"));
}

#[tokio::test]
async fn web_fetcher_tool_call_invalid_url_returns_error() {
    let tool = WebFetcherTool::new();
    let args = json!({"url": "not-a-valid-url"});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn web_fetcher_tool_call_404_returns_error() {
    let tool = WebFetcherTool::new();
    let args = json!({"url": "https://httpbin.org/status/404"});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("404") || err.to_string().contains("status"));
}

#[tokio::test]
async fn web_fetcher_tool_fetches_plain_text() {
    let tool = WebFetcherTool::new();
    let args = json!({"url": "https://httpbin.org/robots.txt"});
    let result = tool.call(args, None).await.unwrap();
    assert!(result.text.contains("User-agent"));
}

#[tokio::test]
async fn web_fetcher_tool_default_construction() {
    let tool = WebFetcherTool::default();
    assert_eq!(tool.name(), TOOL_WEB_FETCHER);
}

#[tokio::test]
async fn web_fetcher_tool_with_custom_client() {
    let client = reqwest::Client::new();
    let tool = WebFetcherTool::with_client(client);
    assert_eq!(tool.name(), TOOL_WEB_FETCHER);
}

/// GET with only url (backward compatibility): same behavior as before.
#[tokio::test]
async fn web_fetcher_tool_call_get_with_only_url() {
    let tool = WebFetcherTool::new();
    let args = json!({"url": "https://httpbin.org/get"});
    let result = tool.call(args, None).await.unwrap();
    assert!(result.text.contains("httpbin.org"));
}

/// POST with JSON body: httpbin.org/post echoes the request.
#[tokio::test]
async fn web_fetcher_tool_call_post_with_json_body() {
    let tool = WebFetcherTool::new();
    let args = json!({
        "url": "https://httpbin.org/post",
        "method": "POST",
        "body": { "hello": "world", "n": 42 }
    });
    let result = tool.call(args, None).await.unwrap();
    assert!(result.text.contains("\"hello\": \"world\""));
    assert!(result.text.contains("\"n\": 42"));
}

/// POST with string body.
#[tokio::test]
async fn web_fetcher_tool_call_post_with_string_body() {
    let tool = WebFetcherTool::new();
    let args = json!({
        "url": "https://httpbin.org/post",
        "method": "POST",
        "body": "plain text body"
    });
    let result = tool.call(args, None).await.unwrap();
    assert!(result.text.contains("plain text body"));
}

/// Unsupported method returns InvalidInput.
#[tokio::test]
async fn web_fetcher_tool_call_unsupported_method_returns_error() {
    let tool = WebFetcherTool::new();
    let args = json!({"url": "https://httpbin.org/get", "method": "PUT"});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("unsupported") || err.to_string().contains("InvalidInput"));
}
