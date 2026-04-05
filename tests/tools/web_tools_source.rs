use loom::tool_source::{ToolSource, WebToolsSource};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn spawn_mock(
    status: u16,
    content_type: &str,
    body: &str,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let ct = content_type.to_string();
    let b = body.to_string();
    let st = status;
    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let _ = stream.read(&mut buf).await;
        let resp = format!(
            "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            st, ct, b.len(), b
        );
        let _ = stream.write_all(resp.as_bytes()).await;
    });
    (format!("http://127.0.0.1:{}", port), handle)
}

#[tokio::test]
async fn web_tools_source_lists_web_fetcher_tool() {
    let source = WebToolsSource::new().await;
    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "web_fetcher");
    assert!(tools[0].description.is_some());
    assert!(tools[0].description.as_ref().unwrap().contains("URL"));
}

#[tokio::test]
async fn web_tools_source_call_web_fetcher_success() {
    let (url, _h) = spawn_mock(200, "application/json", "{\"slideshow\": {\"title\": \"mock\"}}").await;
    let source = WebToolsSource::new().await;
    let args = json!({"url": &url});
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
}

#[tokio::test]
async fn web_tools_source_call_nonexistent_tool() {
    let source = WebToolsSource::new().await;
    let args = json!({"url": "http://127.0.0.1:1"});
    let result = source.call_tool("nonexistent", args).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn web_tools_source_call_tool_with_context() {
    let (url, _h) = spawn_mock(200, "text/plain", "User-agent: *\nDisallow: /").await;
    let source = WebToolsSource::new().await;
    let args = json!({"url": &url});
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
    let (url, _h) = spawn_mock(200, "application/json", "{\"slideshow\": {\"title\": \"mock\"}}").await;
    let client = reqwest::Client::new();
    let source = WebToolsSource::with_client(client).await;
    let args = json!({"url": &url});
    let result = source.call_tool("web_fetcher", args).await.unwrap();
    assert!(!result.text.is_empty());
    assert!(result.text.contains("slideshow"));
}

#[tokio::test]
async fn web_tools_source_fetches_plain_text() {
    let (url, _h) = spawn_mock(200, "text/plain", "User-agent: *\nDisallow: /").await;
    let source = WebToolsSource::new().await;
    let args = json!({"url": &url});
    let result = source.call_tool("web_fetcher", args).await.unwrap();
    assert!(result.text.contains("User-agent"));
}

#[tokio::test]
async fn web_tools_source_handles_404_error() {
    let (url, _h) = spawn_mock(404, "text/plain", "not found").await;
    let source = WebToolsSource::new().await;
    let args = json!({"url": &url});
    let result = source.call_tool("web_fetcher", args).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn web_tools_source_handles_invalid_url() {
    let source = WebToolsSource::new().await;
    let args = json!({"url": "not-a-valid-url"});
    let result = source.call_tool("web_fetcher", args).await;
    assert!(result.is_err());
}
