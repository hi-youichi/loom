//! E2E: run with thread_id then user_messages returns stored user and assistant messages.
//! Uses a mock HTTP server to avoid calling real OpenAI API.

use super::common;
use futures_util::StreamExt;
use loom::protocol::AgentIdentifier;
use loom::{AgentType, ClientRequest, RunRequest, ServerResponse, UserMessagesRequest};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

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

fn is_stream_request(request: &str) -> bool {
    request.contains("\"stream\":true") || request.contains("\"stream\": true")
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
#[allow(clippy::await_holding_lock)]
async fn e2e_user_messages_after_run() {
    let _lock = common::env_test_lock().lock().unwrap();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let db_path = tmp.path().to_string_lossy().to_string();
    let prev_user_message_db = std::env::var("USER_MESSAGE_DB").ok();
    let prev_api_key = std::env::var("OPENAI_API_KEY").ok();
    let prev_api_base = std::env::var("OPENAI_BASE_URL").ok();

    std::env::set_var("USER_MESSAGE_DB", &db_path);
    std::env::set_var("OPENAI_API_KEY", "test-key");

    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_url = format!("http://127.0.0.1:{}", mock_port);
    std::env::set_var("OPENAI_BASE_URL", &mock_url);

    let non_stream_response = serde_json::json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion",
        "created": 1,
        "model": "gpt-4o-mini",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "{\"completed\": true, \"reason\": \"Task finished\"}"
            },
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    })
    .to_string();

    let sse_chunks = vec![
        r#"{"id":"chatcmpl-mock","object":"chat.completion.chunk","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#,
        r#"{"id":"chatcmpl-mock","object":"chat.completion.chunk","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}"#,
        r#"{"id":"chatcmpl-mock","object":"chat.completion.chunk","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
    ];

    let non_stream_resp = non_stream_response.clone();
    let mock_handle = tokio::spawn(async move {
        for _ in 0..10 {
            let (mut stream, _) = match mock_listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
            };
            let request = read_http_request(&mut stream).await;
            if is_stream_request(&request) {
                write_sse_response(&mut stream, &sse_chunks).await;
            } else {
                write_http_response(&mut stream, "200 OK", &non_stream_resp).await;
            }
        }
    });

    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let thread_id = "e2e-um-thread-1";
    let user_msg = "Say exactly: hello from e2e user_messages test.";

    let run_req = ClientRequest::Run(RunRequest {
        id: None,
        message: loom::UserContent::text(user_msg.to_string()),
        agent: AgentIdentifier::Type(AgentType::React),
        thread_id: Some(thread_id.to_string()),
        workspace_id: None,
        working_folder: None,
        got_adaptive: None,
        verbose: Some(false),
        model: None,
    });

    let read_timeout = Duration::from_secs(90);
    let (final_resp, _) =
        common::send_run_and_recv_end(&mut write, &mut read, &run_req, read_timeout)
            .await
            .expect("run should complete");

    match &final_resp {
        ServerResponse::RunEnd(r) => {
            assert!(!r.reply.is_empty(), "run_end reply should be non-empty")
        }
        ServerResponse::Error(e) => panic!("run failed: {}", e.error),
        _ => panic!("expected RunEnd or Error, got {:?}", final_resp),
    }

    let um_req = ClientRequest::UserMessages(UserMessagesRequest {
        id: "um-1".to_string(),
        thread_id: thread_id.to_string(),
        before: None,
        limit: Some(50),
    });
    let (um_resp, _) = common::send_and_recv(&mut write, &mut read, &um_req)
        .await
        .expect("user_messages request should get response");

    match &um_resp {
        ServerResponse::UserMessages(r) => {
            assert_eq!(r.thread_id, thread_id);
            let has_user = r
                .messages
                .iter()
                .any(|m| m.role == "user" && m.content.contains(user_msg));
            let has_assistant = r.messages.iter().any(|m| m.role == "assistant");
            assert!(
                has_user,
                "user_messages should contain the run's user message"
            );
            assert!(
                has_assistant,
                "user_messages should contain at least one assistant message"
            );
        }
        ServerResponse::Error(e) => panic!("user_messages failed: {}", e.error),
        _ => panic!("expected UserMessages or Error, got {:?}", um_resp),
    }

    drop(write);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
    drop(mock_handle);

    match prev_user_message_db {
        Some(v) => std::env::set_var("USER_MESSAGE_DB", v),
        None => std::env::remove_var("USER_MESSAGE_DB"),
    }
    match prev_api_key {
        Some(v) => std::env::set_var("OPENAI_API_KEY", v),
        None => std::env::remove_var("OPENAI_API_KEY"),
    }
    match prev_api_base {
        Some(v) => std::env::set_var("OPENAI_BASE_URL", v),
        None => std::env::remove_var("OPENAI_BASE_URL"),
    }
}
