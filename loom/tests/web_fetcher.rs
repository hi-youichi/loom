//! Integration tests for WebFetcherTool: name, spec, GET/POST call behavior.

use loom::tools::{Tool, WebFetcherTool, TOOL_WEB_FETCHER};
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::method;

async fn spawn_mock(
    status: u16,
    content_type: &str,
    body: &str,
) -> MockServer {
    let server = MockServer::start().await;
    let template = ResponseTemplate::new(status)
        .set_body_string(body)
        .insert_header("content-type", content_type);
    Mock::given(method("GET"))
        .respond_with(template)
        .mount(&server)
        .await;
    server
}

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
    assert!(spec.input_schema.is_object());
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
    let server = spawn_mock(404, "text/plain", "not found").await;
    let tool = WebFetcherTool::new();
    let args = json!({"url": &server.uri()});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn web_fetcher_tool_fetches_plain_text() {
    let server = spawn_mock(200, "text/plain", "User-agent: *\nDisallow: /").await;
    let tool = WebFetcherTool::new();
    let args = json!({"url": &server.uri()});
    let result = tool.call(args, None).await.unwrap();
    assert!(result.as_text().unwrap().contains("User-agent"));
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

#[tokio::test]
async fn web_fetcher_tool_call_get_with_only_url() {
    let server = spawn_mock(200, "application/json", "{\"host\": \"mock-server\"}").await;
    let tool = WebFetcherTool::new();
    let args = json!({"url": &server.uri()});
    let result = tool.call(args, None).await.unwrap();
    assert!(result.as_text().unwrap().contains("mock-server"));
}

#[tokio::test]
async fn web_fetcher_tool_call_post_with_json_body() {
    let body = "{\"hello\": \"world\", \"n\": 42}";
    let server = MockServer::start().await;
    let template = ResponseTemplate::new(200)
        .set_body_string(body)
        .insert_header("content-type", "application/json");
    Mock::given(method("POST"))
        .respond_with(template)
        .mount(&server)
        .await;
    let tool = WebFetcherTool::new();
    let args = json!({
        "url": &server.uri(),
        "method": "POST",
        "body": { "hello": "world", "n": 42 }
    });
    let result = tool.call(args, None).await.unwrap();
    let text = result.as_text().unwrap();
    assert!(text.contains("hello"));
    assert!(text.contains("42"));
}

#[tokio::test]
async fn web_fetcher_tool_call_post_with_string_body() {
    let server = MockServer::start().await;
    let template = ResponseTemplate::new(200)
        .set_body_string("plain text body")
        .insert_header("content-type", "text/plain");
    Mock::given(method("POST"))
        .respond_with(template)
        .mount(&server)
        .await;
    let tool = WebFetcherTool::new();
    let args = json!({
        "url": &server.uri(),
        "method": "POST",
        "body": "plain text body"
    });
    let result = tool.call(args, None).await.unwrap();
    assert!(result.as_text().unwrap().contains("plain text body"));
}

#[tokio::test]
async fn web_fetcher_tool_call_unsupported_method_returns_error() {
    let server = MockServer::start().await;
    let template_get = ResponseTemplate::new(200)
        .set_body_string("ok")
        .insert_header("content-type", "text/plain");
    let template_post = ResponseTemplate::new(200)
        .set_body_string("ok")
        .insert_header("content-type", "text/plain");
    Mock::given(method("GET"))
        .respond_with(template_get)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(template_post)
        .mount(&server)
        .await;
    let tool = WebFetcherTool::new();
    let args = json!({"url": &server.uri(), "method": "PUT"});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("unsupported") || err.to_string().contains("InvalidInput"));
}
