use loom::tool_source::{ToolSource, WebToolsSource};
use serde_json::json;

#[tokio::test]
async fn web_tools_source_lists_web_fetcher_tool() {
    let source = WebToolsSource::new().await;
    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "web_fetcher");
    assert!(tools[0].description.is_some());
    assert!(tools[0]
        .description
        .unwrap()
        .contains("URL"));
}

#[tokio::test]
async fn web_tools_source_call_web_fetcher_success() {
    let source = WebToolsSource::new().await;
    let args = json!({"url": "https://httpbin.org/json"});
    let result = source.call_tool("web_fetcher", args).await.unwrap();
    assert!(!result.text.is_empty());
    assert!(result.text.contains("slideshow"));
}

#[tokio::test]
async fn web_tools_source_call_web_fetcher_missing_url() {
    let source = WebToolsSource::new().await;
    let args = json!({});
    let result = source.call_tool("web_fetcher", args).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("missing") || err.to_string().contains("invalid"));
}

#[tokio::test]
async fn web_tools_source_call_nonexistent_tool() {
    let source = WebToolsSource::new().await;
    let args = json!({"url": "https://example.com"});
    let result = source.call_tool("nonexistent", args).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn web_tools_source_call_tool_with_context() {
    let source = WebToolsSource::new().await;
    let args = json!({"url": "https://httpbin.org/robots.txt"});
    let result = source
        .call_tool_with_context("web_fetcher", args, None)
        .await
        .unwrap();
    assert!(!result.text.is_empty());
    assert!(result.text.contains("User-agent"));
}

#[tokio::test]
async fn web_tools_source_set_call_context() {
    let source = WebToolsSource::new().await;
    source.set_call_context(None);
    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
}

#[tokio::test]
async fn web_tools_source_with_custom_client() {
    let client = reqwest::Client::new();
    let source = WebToolsSource::with_client(client).await;
    let args = json!({"url": "https://httpbin.org/json"});
    let result = source.call_tool("web_fetcher", args).await.unwrap();
    assert!(!result.text.is_empty());
    assert!(result.text.contains("slideshow"));
}

#[tokio::test]
async fn web_tools_source_fetches_plain_text() {
    let source = WebToolsSource::new().await;
    let args = json!({"url": "https://httpbin.org/robots.txt"});
    let result = source.call_tool("web_fetcher", args).await.unwrap();
    assert!(result.text.contains("User-agent"));
}

#[tokio::test]
async fn web_tools_source_handles_404_error() {
    let source = WebToolsSource::new().await;
    let args = json!({"url": "https://httpbin.org/status/404"});
    let result = source.call_tool("web_fetcher", args).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("404") || err.to_string().contains("status"));
}

#[tokio::test]
async fn web_tools_source_handles_invalid_url() {
    let source = WebToolsSource::new().await;
    let args = json!({"url": "not-a-valid-url"});
    let result = source.call_tool("web_fetcher", args).await;
    assert!(result.is_err());
}
