//! Test for X-Request-Id header functionality
//! 
//! This test verifies that ChatOpenAICompat correctly generates and sends
//! X-Request-Id headers with unique values for each request.

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
                    if lower.starts_with("content-length:") {
                        line.strip_prefix("content-length:")
                            .or(line.strip_prefix("Content-Length:"))
                            .map(|s| s.trim().parse::<usize>().ok())
                            .flatten()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);

            while buf.len() < header_end + content_length {
                let n = stream.read(&mut tmp).await.unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            return String::from_utf8_lossy(&buf.as_slice()).to_string();
        }
    }
    String::from_utf8_lossy(&buf.as_slice()).to_string()
}

#[tokio::test]
async fn test_chat_openai_compat_sends_request_id() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;

        assert!(request.contains("x-request-id:"), 
                "Request should contain X-Request-Id header");

        let response = r#"{
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "test-model",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        }"#;

        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ").await.unwrap();
        stream.write_all(response.len().to_string().as_bytes()).await.unwrap();
        stream.write_all(b"\r\n\r\n").await.unwrap();
        stream.write_all(response.as_bytes()).await.unwrap();
    });

    let client = ChatOpenAICompat::with_config(
        format!("http://{}", addr),
        "test-key",
        "gpt-4",
    );

    let messages = vec![Message::user("Hello!".to_string())];
    let _result = client.invoke(&messages).await.unwrap();
}

#[tokio::test]
async fn test_chat_openai_compat_generates_unique_request_ids() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut request_ids = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let request_ids_clone = request_ids.clone();

    tokio::spawn(async move {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;

            if let Some(request_id) = request.lines()
                .find(|line| line.to_ascii_lowercase().starts_with("x-request-id:"))
                .and_then(|line| line.split(':').nth(1).map(|s| s.trim().to_string())) {
                request_ids_clone.lock().unwrap().push(request_id);
            }

            let response = r#"{
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 1234567890,
                "model": "test-model",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Response"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15
                }
            }"#;

            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ").await.unwrap();
            stream.write_all(response.len().to_string().as_bytes()).await.unwrap();
            stream.write_all(b"\r\n\r\n").await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });

    let client = ChatOpenAICompat::with_config(
        format!("http://{}", addr),
        "test-key",
        "gpt-4",
    );

    let messages = vec![Message::user("First request".to_string())];
    let _result1 = client.invoke(&messages).await.unwrap();

    let messages = vec![Message::user("Second request".to_string())];
    let _result2 = client.invoke(&messages).await.unwrap();

    let ids = request_ids.lock().unwrap();
    assert_eq!(ids.len(), 2, "Should have captured 2 request IDs");
    assert_ne!(ids[0], ids[1], "Request IDs should be unique for each request");
}

#[tokio::test]
async fn test_chat_openai_compat_request_id_with_other_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;

        assert!(request.contains("x-request-id:"), 
                "Request should contain X-Request-Id header");
        assert!(request.contains("x-app-id:"), 
                "Request should contain X-App-Id header");
        assert!(request.contains("x-thread-id:"), 
                "Request should contain X-Thread-Id header when set");

        let response = r#"{
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "test-model",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        }"#;

        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ").await.unwrap();
        stream.write_all(response.len().to_string().as_bytes()).await.unwrap();
        stream.write_all(b"\r\n\r\n").await.unwrap();
        stream.write_all(response.as_bytes()).await.unwrap();
    });

    let headers = LlmHeaders::default()
        .with_thread_id("test-thread-123")
        .with_trace_id("test-trace-456");

    let client = ChatOpenAICompat::with_config(
        format!("http://{}", addr),
        "test-key",
        "gpt-4",
    ).with_headers(headers);

    let messages = vec![Message::user("Hello!".to_string())];
    let _result = client.invoke(&messages).await.unwrap();
}