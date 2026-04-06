//! Runs the React agent via the server using a mock LLM server.

use super::common;
use futures_util::{SinkExt, StreamExt};
use loom::{AgentType, ClientRequest, ProtocolEvent, RunRequest, ServerResponse};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

fn assert_non_empty(field: &str, value: &str) {
    assert!(
        !value.trim().is_empty(),
        "expected non-empty {}, got {:?}",
        field,
        value
    );
}

fn assert_optional_non_empty(field: &str, value: &Option<String>) {
    if let Some(v) = value {
        assert_non_empty(field, v);
    }
}

fn assert_event(event: &ProtocolEvent, saw_node_enter: &mut bool, saw_node_exit: &mut bool) {
    match event {
        ProtocolEvent::NodeEnter { id } => {
            assert_non_empty("event.id", id);
            *saw_node_enter = true;
        }
        ProtocolEvent::NodeExit { id, result: _ } => {
            assert_non_empty("event.id", id);
            *saw_node_exit = true;
        }
        ProtocolEvent::ToolCall {
            call_id,
            name,
            arguments,
        } => {
            assert_optional_non_empty("event.call_id", call_id);
            assert_non_empty("event.name", name);
            assert!(arguments.is_object());
        }
        ProtocolEvent::ToolCallChunk {
            call_id,
            name,
            arguments_delta: _,
        } => {
            assert_optional_non_empty("event.call_id", call_id);
            assert_optional_non_empty("event.name", name);
        }
        ProtocolEvent::MessageChunk { content, id: _ } => {
            assert_non_empty("event.content", content);
        }
        ProtocolEvent::ThoughtChunk { content, id: _ } => {
            assert_non_empty("event.content", content);
        }
        _ => {}
    }
}

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
        status, body.len(), body
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
        body.len(), body
    );
    stream.write_all(resp.as_bytes()).await.unwrap();
}

#[tokio::test]
async fn e2e_run_then_disconnect() {
    common::load_dotenv();
    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, read) = ws.split();

    let req = ClientRequest::Run(RunRequest {
        id: None,
        message: loom::UserContent::text("hi".to_string()),
        agent: AgentType::React,
        thread_id: None,
        workspace_id: None,
        working_folder: None,
        got_adaptive: None,
        verbose: Some(false),
    });
    let req_json = serde_json::to_string(&req).unwrap();
    write.send(Message::Text(req_json)).await.unwrap();
    drop(write);
    drop(read);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}

#[tokio::test]
async fn e2e_run_react() {
    let _lock = common::env_test_lock().lock().unwrap();
    common::load_dotenv();

    let prev_api_key = std::env::var("OPENAI_API_KEY").ok();
    let prev_base_url = std::env::var("OPENAI_BASE_URL").ok();

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
        r#"{"id":"chatcmpl-mock","object":"chat.completion.chunk","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"content":"Hello from mock LLM"},"finish_reason":null}]}"#,
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

    let req = ClientRequest::Run(RunRequest {
        id: None,
        message: loom::UserContent::text("Say hello".to_string()),
        agent: AgentType::React,
        thread_id: None,
        workspace_id: None,
        working_folder: None,
        got_adaptive: None,
        verbose: Some(false),
    });
    let read_timeout = Duration::from_secs(30);
    let req_json = serde_json::to_string(&req).unwrap();
    write.send(Message::Text(req_json)).await.unwrap();

    let mut saw_node_enter = false;
    let mut saw_node_exit = false;

    let _resp = loop {
        let opt = timeout(read_timeout, read.next())
            .await
            .expect("timeout waiting for run response");
        let msg_result = opt.expect("no message");
        let msg = msg_result.expect("websocket error");
        let text = msg.to_text().expect("not text");
        eprintln!("[e2e] received: {}", text);
        let server_resp: ServerResponse = serde_json::from_str(text).expect("parse");

        match server_resp {
            ServerResponse::RunStreamEvent(ev) => {
                assert_event(&ev.event.event, &mut saw_node_enter, &mut saw_node_exit);
            }
            ServerResponse::RunEnd(r) => {
                assert_non_empty("run_end.reply", &r.reply);
                break ServerResponse::RunEnd(r);
            }
            ServerResponse::Error(e) => {
                panic!("server run error: {} (id={:?})", e.error, e.id);
            }
            _ => continue,
        }
    };

    assert!(saw_node_enter, "expected at least one node_enter event");
    assert!(saw_node_exit, "expected at least one node_exit event");

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
    drop(mock_handle);

    match prev_api_key {
        Some(v) => std::env::set_var("OPENAI_API_KEY", v),
        None => std::env::remove_var("OPENAI_API_KEY"),
    }
    match prev_base_url {
        Some(v) => std::env::set_var("OPENAI_BASE_URL", v),
        None => std::env::remove_var("OPENAI_BASE_URL"),
    }
}
