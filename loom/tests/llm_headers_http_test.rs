//! Integration tests for LLM HTTP headers transmission verification
//! 
//! This module tests that ChatOpenAICompat correctly sends custom headers
//! (X-Thread-Id, X-Trace-Id, X-App-Id, etc.) to the HTTP API endpoint
//! using a mock HTTP server.

use loom::llm::{ChatOpenAICompat, LlmClient, LlmHeaders};
use loom::Message;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        let n = stream.read(&mut tmp).await.unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let header_end = pos + 4;
            let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            while buf.len() < header_end + content_length {
                let m = stream.read(&mut tmp).await.unwrap();
                if m == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..m]);
            }
            return String::from_utf8_lossy(&buf).to_string();
        }
    }
    String::new()
}

async fn write_http_response(stream: &mut tokio::net::TcpStream, status: &str, body: &str) {
    let resp = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        status,
        body.len(),
        body
    );
    stream.write_all(resp.as_bytes()).await.unwrap();
}

async fn write_sse_response(stream: &mut tokio::net::TcpStream, chunks: &[&str]) {
    let mut body = String::new();
    for chunk in chunks {
        body.push_str(&format!("data: {}\n\n", chunk));
    }
    body.push_str("data: [DONE]\n\n");
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(resp.as_bytes()).await.unwrap();
}

#[tokio::test]
async fn chat_openai_compat_sends_x_thread_id() {
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_url = format!("http://127.0.0.1:{}", mock_port);
    
    let expected_thread_id = "test-thread-12345";
    let expected_trace_id = "test-trace-67890";
    
    let mock_handle = tokio::spawn(async move {
        let (mut stream, _) = mock_listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        
        assert!(request.contains(&format!("x-thread-id: {}", expected_thread_id)), 
                "Request should contain X-Thread-Id header");
        assert!(request.contains(&format!("x-trace-id: {}", expected_trace_id)), 
                "Request should contain X-Trace-Id header");
        assert!(request.contains("x-app-id: loom"), 
                "Request should contain X-App-Id header");
        
        let response = r#"{
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "test response"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        }"#;
        write_http_response(&mut stream, "200 OK", response).await;
    });
    
    let headers = LlmHeaders::default()
        .with_thread_id(expected_thread_id)
        .with_trace_id(expected_trace_id);
    
    let client = ChatOpenAICompat::with_config(
        &mock_url, 
        "test-key", 
        "gpt-4"
    ).with_headers(headers);
    
    let messages = vec![Message::user("test message")];
    let response = client.invoke(&messages).await;
    
    assert!(response.is_ok(), "Client request should succeed");
    mock_handle.await.unwrap();
}

#[tokio::test]
async fn chat_openai_compat_sends_custom_headers() {
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_url = format!("http://127.0.0.1:{}", mock_port);
    
    let expected_custom_value = "custom-value-123";
    
    let mock_handle = tokio::spawn(async move {
        let (mut stream, _) = mock_listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        
        assert!(request.contains(&format!("x-custom-header: {}", expected_custom_value)), 
                "Request should contain X-Custom-Header header");
        assert!(request.contains("x-app-id: loom"), 
                "Request should contain X-App-Id header");
        
        let response = r#"{
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "response"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        }"#;
        write_http_response(&mut stream, "200 OK", response).await;
    });
    
    let headers = LlmHeaders::default()
        .add_header("X-Custom-Header", expected_custom_value);
    
    let client = ChatOpenAICompat::with_config(
        &mock_url, 
        "test-key", 
        "gpt-4"
    ).with_headers(headers);
    
    let messages = vec![Message::user("test")];
    let response = client.invoke(&messages).await;
    
    assert!(response.is_ok(), "Client request should succeed");
    mock_handle.await.unwrap();
}

#[tokio::test]
async fn chat_openai_compat_sends_all_headers_combined() {
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_url = format!("http://127.0.0.1:{}", mock_port);
    
    let expected_thread_id = "combined-thread-123";
    let expected_trace_id = "combined-trace-456";
    let expected_custom_1 = "value-1";
    let expected_custom_2 = "value-2";
    
    let mock_handle = tokio::spawn(async move {
        let (mut stream, _) = mock_listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        
        assert!(request.contains(&format!("x-thread-id: {}", expected_thread_id)), 
                "Request should contain X-Thread-Id header");
        assert!(request.contains(&format!("x-trace-id: {}", expected_trace_id)), 
                "Request should contain X-Trace-Id header");
        assert!(request.contains("x-app-id: loom"), 
                "Request should contain X-App-Id header");
        assert!(request.contains(&format!("x-custom-1: {}", expected_custom_1)), 
                "Request should contain X-Custom-1 header");
        assert!(request.contains(&format!("x-custom-2: {}", expected_custom_2)), 
                "Request should contain X-Custom-2 header");
        
        let response = r#"{
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "response"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        }"#;
        write_http_response(&mut stream, "200 OK", response).await;
    });
    
    let headers = LlmHeaders::default()
        .with_thread_id(expected_thread_id)
        .with_trace_id(expected_trace_id)
        .add_header("X-Custom-1", expected_custom_1)
        .add_header("X-Custom-2", expected_custom_2);
    
    let client = ChatOpenAICompat::with_config(
        &mock_url, 
        "test-key", 
        "gpt-4"
    ).with_headers(headers);
    
    let messages = vec![Message::user("test")];
    let response = client.invoke(&messages).await;
    
    assert!(response.is_ok(), "Client request should succeed");
    mock_handle.await.unwrap();
}

#[tokio::test]
async fn chat_openai_compat_stream_request_sends_headers() {
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_url = format!("http://127.0.0.1:{}", mock_port);
    
    let expected_thread_id = "stream-thread-789";
    
    let mock_handle = tokio::spawn(async move {
        let (mut stream, _) = mock_listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        
        assert!(request.contains(&format!("x-thread-id: {}", expected_thread_id)), 
                "Stream request should contain X-Thread-Id header");
        assert!(request.contains("x-app-id: loom"), 
                "Stream request should contain X-App-Id header");
        assert!(request.contains("\"stream\":true") || request.contains("\"stream\": true"),
                "Request should have stream parameter set to true");
        
        let sse_chunks = vec![
            r#"{"id":"chatcmpl-test","object":"chat.completion.chunk","created":1,"model":"gpt-4","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#,
            r#"{"id":"chatcmpl-test","object":"chat.completion.chunk","created":1,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}"#,
            r#"{"id":"chatcmpl-test","object":"chat.completion.chunk","created":1,"model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        ];
        write_sse_response(&mut stream, &sse_chunks).await;
    });
    
    let headers = LlmHeaders::default()
        .with_thread_id(expected_thread_id);
    
    let client = ChatOpenAICompat::with_config(
        &mock_url, 
        "test-key", 
        "gpt-4"
    ).with_headers(headers);
    
    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel(10);
    let messages = vec![Message::user("test")];
    
    let handle = tokio::spawn(async move {
        client.invoke_stream(&messages, Some(chunk_tx)).await
    });
    
    let mut received_chunks = 0;
    while let Some(_chunk) = chunk_rx.recv().await {
        received_chunks += 1;
    }
    
    assert!(received_chunks > 0, "Should receive at least one chunk");
    
    let response = handle.await.unwrap();
    assert!(response.is_ok(), "Stream request should succeed");
    mock_handle.await.unwrap();
}

#[tokio::test]
async fn chat_openai_compat_without_headers_no_custom_headers_sent() {
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_url = format!("http://127.0.0.1:{}", mock_port);
    
    let mock_handle = tokio::spawn(async move {
        let (mut stream, _) = mock_listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        
        assert!(!request.contains("x-thread-id:"), 
                "Request should NOT contain X-Thread-Id header when not set");
        assert!(!request.contains("x-trace-id:"), 
                "Request should NOT contain X-Trace-Id header when not set");
        assert!(!request.contains("x-custom-"), 
                "Request should NOT contain custom headers when not set");
        assert!(!request.contains("x-app-id:"), 
                "Request should NOT contain X-App-Id header when headers not configured");
        
        let response = r#"{
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "response"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        }"#;
        write_http_response(&mut stream, "200 OK", response).await;
    });
    
    let client = ChatOpenAICompat::with_config(
        &mock_url, 
        "test-key", 
        "gpt-4"
    );
    
    let messages = vec![Message::user("test")];
    let response = client.invoke(&messages).await;
    
    assert!(response.is_ok(), "Client request should succeed");
    mock_handle.await.unwrap();
}