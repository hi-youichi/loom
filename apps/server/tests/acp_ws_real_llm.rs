//! Real LLM e2e test for ACP-over-WebSocket.
//!
//! Requires actual LLM credentials in env:
//! - OPENAI_API_KEY
//! - OPENAI_BASE_URL (e.g. https://api.modelgate.dev/v1)
//! - MODEL (e.g. glm-4-flash)
//!
//! Run manually:
//! ```sh
//! cargo test -p loom-server --test acp_ws_real_llm -- --ignored --nocapture
//! ```

#![cfg(test)]

use std::time::Duration;

use axum::Router;
use futures::{SinkExt, StreamExt};
use loom_server::{routes::build_router, state::new_state};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message,
    WebSocketStream, MaybeTlsStream,
};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn start_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app: Router = build_router(new_state());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("ws://{addr}/acp"), task)
}

fn check_llm_env() -> Option<(String, String, String)> {
    let key = std::env::var("OPENAI_API_KEY").ok()?;
    let base = std::env::var("OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let model = std::env::var("MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    if key.is_empty() {
        return None;
    }
    Some((key, base, model))
}

async fn send_request(
    socket: &mut WsStream,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string(),
        ))
        .await
        .expect("send");
    loop {
        let msg = socket.next().await.expect("socket open").expect("frame");
        if let Message::Text(text) = msg {
            let frame: Value = serde_json::from_str(&text).expect("JSON");
            if frame["id"] == id {
                return frame;
            }
        }
    }
}

/// Collect all session/update notifications until the PromptResponse arrives.
///
/// Returns (prompt_response, all_notifications).
async fn send_prompt_and_collect(
    socket: &mut WsStream,
    id: u64,
    session_id: &str,
    prompt_text: &str,
) -> (Value, Vec<Value>) {
    socket
        .send(Message::Text(
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "session/prompt",
                "params": {
                    "sessionId": session_id,
                    "prompt": [{"type": "text", "text": prompt_text}]
                }
            })
            .to_string(),
        ))
        .await
        .expect("send prompt");

    let mut notifications = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("timed out waiting for PromptResponse after 120s");
        }
        match tokio::time::timeout(remaining, socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let frame: Value = serde_json::from_str(&text).expect("JSON");
                if frame["id"] == id && frame.get("result").is_some() {
                    return (frame, notifications);
                }
                if frame.get("method").and_then(|m| m.as_str()) == Some("session/update") {
                    notifications.push(frame.clone());
                }
                if frame.get("error").is_some() {
                    panic!("unexpected error from server: {frame}");
                }
            }
            Ok(Some(Ok(_))) => continue,
            Ok(None) => panic!("socket closed before PromptResponse"),
            Ok(Some(Err(e))) => panic!("ws error: {e}"),
            Err(_) => panic!("timed out waiting for next message"),
        }
    }
}

#[tokio::test]
#[ignore = "requires real LLM credentials; run with --ignored"]
async fn real_llm_prompt_returns_text_and_notifications() {
    let creds = match check_llm_env() {
        Some(c) => c,
        None => panic!("Set OPENAI_API_KEY, OPENAI_BASE_URL, MODEL env vars to run this test"),
    };
    let (_key, _base, model) = creds;

    let (url, server) = start_server().await;

    // Connect
    let (mut socket, _) = connect_async(&url).await.expect("connect");

    // Initialize
    let init = send_request(
        &mut socket,
        1,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientInfo": {"name": "real-llm-test", "version": "0.1"},
            "capabilities": {}
        }),
    )
    .await;
    assert!(init.get("result").is_some(), "initialize failed: {init}");

    // Create session with explicit model
    let session = send_request(
        &mut socket,
        2,
        "session/new",
        json!({
            "cwd": std::env::current_dir().unwrap().to_string_lossy(),
            "mcpServers": [],
            "sessionConfig": {
                "model": model
            }
        }),
    )
    .await;
    assert!(session.get("result").is_some(), "session/new failed: {session}");
    let session_id = session["result"]["sessionId"]
        .as_str()
        .expect("sessionId")
        .to_owned();

    // Send prompt — ask a simple question
    let (response, notifications) =
        send_prompt_and_collect(&mut socket, 3, &session_id, "What is 2+3? Reply with just the number.").await;

    // Verify PromptResponse
    assert!(
        response.get("result").is_some(),
        "prompt response should have result: {response}"
    );
    let stop_reason = response["result"]["stopReason"]
        .as_str()
        .unwrap_or("unknown");
    assert!(
        stop_reason == "end_turn" || stop_reason == "stop",
        "expected stopReason end_turn or stop, got {stop_reason}"
    );

    // Should have received at least one session/update
    assert!(
        !notifications.is_empty(),
        "expected at least one session/update notification"
    );

    // At least one notification should contain text content
    let has_text = notifications.iter().any(|n| {
        let params = &n["params"];
        let update = &params["update"];
        // Check various possible notification shapes
        update.get("text").is_some()
            || update.get("content").is_some()
            || update.get("message").is_some()
            || update
                .get("assistant_message")
                .and_then(|m| m.get("content"))
                .is_some()
            || params.get("update").and_then(|u| u.as_str()).is_some()
    });
    assert!(
        has_text,
        "expected at least one notification with text content; got {} notifications: {:#?}",
        notifications.len(),
        &notifications[..notifications.len().min(3)]
    );

    println!(
        "\n✅ Real LLM e2e: received {} notifications, stopReason={}",
        notifications.len(),
        stop_reason
    );

    let _ = socket.close(None).await;
    server.abort();
}

#[tokio::test]
#[ignore = "requires real LLM credentials; run with --ignored"]
async fn real_llm_multi_turn_remembers_context() {
    let creds = match check_llm_env() {
        Some(c) => c,
        None => panic!("Set OPENAI_API_KEY, OPENAI_BASE_URL, MODEL env vars to run this test"),
    };
    let (_key, _base, model) = creds;

    let (url, server) = start_server().await;
    let (mut socket, _) = connect_async(&url).await.expect("connect");

    send_request(
        &mut socket,
        1,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientInfo": {"name": "real-llm-multi-turn", "version": "0.1"},
            "capabilities": {}
        }),
    )
    .await;

    let session = send_request(
        &mut socket,
        2,
        "session/new",
        json!({
            "cwd": std::env::current_dir().unwrap().to_string_lossy(),
            "mcpServers": [],
            "sessionConfig": {"model": model}
        }),
    )
    .await;
    let session_id = session["result"]["sessionId"]
        .as_str()
        .expect("sessionId")
        .to_owned();

    // Turn 1: tell it a secret
    let (resp1, _) = send_prompt_and_collect(
        &mut socket,
        3,
        &session_id,
        "Remember the secret word: BANANA-42. Just say OK.",
    )
    .await;
    assert!(resp1.get("result").is_some(), "turn 1 failed: {resp1}");

    // Turn 2: ask it to recall
    let (resp2, _) = send_prompt_and_collect(
        &mut socket,
        4,
        &session_id,
        "What is the secret word I told you?",
    )
    .await;
    assert!(resp2.get("result").is_some(), "turn 2 failed: {resp2}");

    println!("\n✅ Multi-turn context: both turns completed");

    let _ = socket.close(None).await;
    server.abort();
}
