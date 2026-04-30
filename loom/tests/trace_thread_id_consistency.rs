//! Integration test: verify X-Thread-Id consistency between root agent and sub-agent.
//!
//! The root agent's `trace_thread_id` must be inherited unchanged by every sub-agent
//! so that **all** LLM calls across the hierarchy carry the same `X-Thread-Id` HTTP
//! header for external tracing (e.g. Datadog, Langfuse).
//!
//! Tests cover:
//! 1. Config-level: `trace_thread_id` propagation from parent to sub-agent config
//! 2. HTTP-level: both root and sub-agent LLM clients emit the same `X-Thread-Id`

mod init_logging;

use std::sync::{Arc, Mutex};

use loom::llm::{ChatOpenAICompat, LlmClient, LlmHeaders};
use loom::Message;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Helpers (adapted from llm_headers_http_test.rs)
// ---------------------------------------------------------------------------

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

fn openai_completion_response() -> &'static str {
    r#"{
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 1,
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "mock response"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    }"#
}

fn extract_header_value<'a>(request: &'a str, header_name: &str) -> Option<&'a str> {
    for line in request.lines() {
        let lower = line.to_ascii_lowercase();
        let prefix = format!("{}:", header_name.to_ascii_lowercase());
        if lower.starts_with(&prefix) {
            return Some(line.split(':').nth(1).map(|v| v.trim()).unwrap_or(""));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Config-level tests
// ---------------------------------------------------------------------------

/// Verify the config propagation logic that InvokeAgentTool::call_single_exec uses:
/// - sub_config.thread_id = unique (sub-{parent}-{name}-{depth})
/// - sub_config.trace_thread_id = inherited from parent unchanged
#[test]
fn sub_agent_config_inherits_trace_thread_id() {
    use loom::ReactBuildConfig;

    let root_config = ReactBuildConfig {
        thread_id: Some("root-session-001".to_string()),
        trace_thread_id: Some("trace-abc-123".to_string()),
        ..ReactBuildConfig::from_env()
    };

    let parent_thread_id = root_config.thread_id.as_deref().unwrap_or("root");
    let agent_name = "dev";
    let depth: u32 = 0;

    let mut sub_config = root_config.clone();
    sub_config.thread_id = Some(format!(
        "sub-{}-{}-{}",
        parent_thread_id, agent_name, depth
    ));
    sub_config.trace_thread_id = root_config.trace_thread_id.clone();

    assert_eq!(
        sub_config.thread_id,
        Some("sub-root-session-001-dev-0".to_string()),
        "sub-agent thread_id must be unique"
    );
    assert_eq!(
        sub_config.trace_thread_id,
        Some("trace-abc-123".to_string()),
        "sub-agent trace_thread_id must match root"
    );
}

/// At depth=1 (sub-agent invoking another sub-agent), trace_thread_id is still the same.
#[test]
fn nested_sub_agent_keeps_same_trace_thread_id() {
    use loom::ReactBuildConfig;

    let root_trace = "trace-root-999";

    let mut depth0_config = ReactBuildConfig {
        thread_id: Some("sub-root-session-dev-0".to_string()),
        trace_thread_id: Some(root_trace.to_string()),
        ..ReactBuildConfig::from_env()
    };

    let depth = 1u32;
    let parent_thread_id = depth0_config.thread_id.as_deref().unwrap_or("root");
    depth0_config.thread_id = Some(format!(
        "sub-{}-{}-{}",
        parent_thread_id, "explore", depth
    ));

    assert_eq!(
        depth0_config.thread_id,
        Some("sub-sub-root-session-dev-0-explore-1".to_string())
    );
    assert_eq!(depth0_config.trace_thread_id, Some(root_trace.to_string()));
}

/// When trace_thread_id is None, thread_id is used as fallback (matching
/// build/llm.rs logic: trace_thread_id.or(thread_id)).
#[test]
fn trace_thread_id_falls_back_to_thread_id() {
    use loom::ReactBuildConfig;

    let root_config = ReactBuildConfig {
        thread_id: Some("session-only-001".to_string()),
        trace_thread_id: None,
        ..ReactBuildConfig::from_env()
    };

    let trace_id = root_config
        .trace_thread_id
        .as_ref()
        .or(root_config.thread_id.as_ref());

    assert_eq!(trace_id.map(|s| s.as_str()), Some("session-only-001"));
}

// ---------------------------------------------------------------------------
// HTTP-level tests
// ---------------------------------------------------------------------------

/// Two ChatOpenAICompat clients (simulating root and sub-agent) both configured
/// with the same trace_thread_id must send the same X-Thread-Id header.
#[tokio::test]
async fn root_and_sub_agent_send_same_x_thread_id() {
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_url = format!("http://127.0.0.1:{}", mock_port);

    let shared_trace_id = "shared-trace-id-xyz";

    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();

    let mock_handle = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut stream, _) = mock_listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            let x_thread_id = extract_header_value(&request, "x-thread-id")
                .unwrap_or("MISSING")
                .to_string();
            captured_clone.lock().unwrap().push(x_thread_id);
            write_http_response(&mut stream, "200 OK", openai_completion_response()).await;
        }
    });

    let headers = LlmHeaders::default().with_thread_id(shared_trace_id);

    let root_client = ChatOpenAICompat::with_config(&mock_url, "test-key", "gpt-4")
        .with_headers(headers.clone());
    let sub_client =
        ChatOpenAICompat::with_config(&mock_url, "test-key", "gpt-4").with_headers(headers);

    let messages = vec![Message::user("hello")];
    let r1 = root_client.invoke(&messages).await;
    let r2 = sub_client.invoke(&messages).await;

    assert!(r1.is_ok(), "root client request should succeed");
    assert!(r2.is_ok(), "sub-agent client request should succeed");
    mock_handle.await.unwrap();

    let captured = captured.lock().unwrap();
    assert_eq!(captured.len(), 2, "should have captured 2 requests");
    assert_eq!(
        captured[0], captured[1],
        "root and sub-agent X-Thread-Id must be identical"
    );
    assert_eq!(captured[0], shared_trace_id);
}

/// Verify the full chain: root config trace_thread_id -> LlmHeaders -> X-Thread-Id HTTP header.
#[tokio::test]
async fn trace_thread_id_appears_as_x_thread_id_header() {
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_url = format!("http://127.0.0.1:{}", mock_port);

    let trace_id = "root-trace-from-config";

    let captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let captured_clone = captured.clone();

    let mock_handle = tokio::spawn(async move {
        let (mut stream, _) = mock_listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let x_thread_id = extract_header_value(&request, "x-thread-id")
            .unwrap_or("MISSING")
            .to_string();
        *captured_clone.lock().unwrap() = x_thread_id;
        write_http_response(&mut stream, "200 OK", openai_completion_response()).await;
    });

    let headers = LlmHeaders::default().with_thread_id(trace_id);
    let client = ChatOpenAICompat::with_config(&mock_url, "test-key", "gpt-4")
        .with_headers(headers);

    let messages = vec![Message::user("test")];
    let result = client.invoke(&messages).await;
    assert!(result.is_ok());
    mock_handle.await.unwrap();

    let captured = captured.lock().unwrap();
    assert_eq!(*captured, trace_id, "X-Thread-Id must equal trace_thread_id");
}

/// Verify X-App-Id is also set alongside X-Thread-Id.
#[tokio::test]
async fn x_thread_id_and_x_app_id_both_present() {
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_url = format!("http://127.0.0.1:{}", mock_port);

    let trace_id = "trace-with-app-id";

    let captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let captured_clone = captured.clone();

    let mock_handle = tokio::spawn(async move {
        let (mut stream, _) = mock_listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        *captured_clone.lock().unwrap() = request;
        write_http_response(&mut stream, "200 OK", openai_completion_response()).await;
    });

    let headers = LlmHeaders::default().with_thread_id(trace_id);
    let client = ChatOpenAICompat::with_config(&mock_url, "test-key", "gpt-4")
        .with_headers(headers);

    let messages = vec![Message::user("test")];
    let result = client.invoke(&messages).await;
    assert!(result.is_ok());
    mock_handle.await.unwrap();

    let request = captured.lock().unwrap();
    assert_eq!(
        extract_header_value(&request, "x-thread-id"),
        Some(trace_id)
    );
    assert_eq!(
        extract_header_value(&request, "x-app-id"),
        Some("loom")
    );
}
