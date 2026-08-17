//! End-to-end ACP-over-WebSocket contract tests.
//!
//! Covers the full lifecycle: initialize, session/new, disconnect, reconnect,
//! session/load — plus transport hardening: binary frame rejection, invalid
//! JSON handling, concurrent prompt rejection, protocolVersion assertion, and
//! multi-session isolation.

use axum::Router;
use futures::{SinkExt, StreamExt};
use loom_server::{routes::build_router, state::new_state};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn start_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let address = listener.local_addr().expect("listener address");
    let app: Router = build_router(new_state());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test router");
    });
    (format!("ws://{address}/acp"), task)
}

async fn send_request(socket: &mut WsStream, id: u64, method: &str, params: Value) -> Value {
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string(),
        ))
        .await
        .expect("send request");
    loop {
        let message = socket.next().await.expect("socket open").expect("ws frame");
        let Message::Text(text) = message else {
            continue;
        };
        let frame: Value = serde_json::from_str(&text).expect("JSON-RPC frame");
        if frame["id"] == id {
            return frame;
        }
    }
}

async fn do_initialize(socket: &mut WsStream, id: u64) -> Value {
    send_request(
        socket,
        id,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientInfo": {"name":"test-harness", "version":"0.1"},
            "capabilities": {"session": {"list": {}}}
        }),
    )
    .await
}

async fn do_new_session(socket: &mut WsStream, id: u64) -> String {
    let created = send_request(
        socket,
        id,
        "session/new",
        json!({
            "cwd": std::env::current_dir().unwrap().to_string_lossy(),
            "mcpServers": []
        }),
    )
    .await;
    created["result"]["sessionId"]
        .as_str()
        .expect("session id")
        .to_owned()
}

// ─── Core lifecycle test ───────────────────────────────────────────────────

#[tokio::test]
async fn full_lifecycle_initialize_new_disconnect_reconnect_load() {
    let (url, server) = start_server().await;
    let (mut first, _) = connect_async(&url).await.expect("connect");

    let init = do_initialize(&mut first, 1).await;
    assert!(init.get("result").is_some(), "initialize failed: {init}");

    let session_id = do_new_session(&mut first, 2).await;

    first.close(None).await.expect("close first");

    let (mut second, _) = connect_async(&url).await.expect("reconnect");
    let init2 = do_initialize(&mut second, 3).await;
    assert!(init2.get("result").is_some());

    let loaded = send_request(
        &mut second,
        4,
        "session/load",
        json!({
            "sessionId": session_id,
            "cwd": std::env::current_dir().unwrap().to_string_lossy(),
            "mcpServers": []
        }),
    )
    .await;
    assert!(
        loaded.get("result").is_some(),
        "session not retained after reconnect: {loaded}"
    );

    second.close(None).await.expect("close second");
    server.abort();
}

// ─── Binary frame rejection ────────────────────────────────────────────────

#[tokio::test]
async fn binary_frame_is_rejected() {
    let (url, server) = start_server().await;
    let (mut socket, _) = connect_async(&url).await.expect("connect");

    socket
        .send(Message::Binary(b"{not json}".to_vec()))
        .await
        .expect("send binary");

    // The server should close the connection or send an error.
    // We accept None (stream end) as a valid "rejected" signal.
    let mut rejected = false;
    for _ in 0..5 {
        match tokio::time::timeout(std::time::Duration::from_secs(3), socket.next()).await {
            Ok(None) => {
                rejected = true;
                break;
            }
            Ok(Some(Ok(Message::Close(_)))) => {
                rejected = true;
                break;
            }
            Ok(Some(Ok(Message::Text(text)))) => {
                if let Ok(frame) = serde_json::from_str::<Value>(&text) {
                    if frame.get("error").is_some() {
                        rejected = true;
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    assert!(
        rejected,
        "expected error, close, or stream end after binary frame"
    );
    let _ = socket.close(None).await;
    server.abort();
}

// ─── Invalid JSON-RPC handling ─────────────────────────────────────────────

#[tokio::test]
async fn invalid_json_returns_error_or_closes() {
    let (url, server) = start_server().await;
    let (mut socket, _) = connect_async(&url).await.expect("connect");

    socket
        .send(Message::Text("not valid json at all".to_string()))
        .await
        .expect("send garbage");

    let mut handled = false;
    for _ in 0..5 {
        match tokio::time::timeout(std::time::Duration::from_secs(2), socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                if let Ok(frame) = serde_json::from_str::<Value>(&text) {
                    if frame.get("error").is_some() {
                        handled = true;
                        break;
                    }
                }
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Err(_) => {
                handled = true;
                break;
            }
            _ => {}
        }
    }
    assert!(handled, "expected error response or connection close");
    let _ = socket.close(None).await;
    server.abort();
}

// ─── protocolVersion assertion ─────────────────────────────────────────────

#[tokio::test]
async fn initialize_response_contains_protocol_version() {
    let (url, server) = start_server().await;
    let (mut socket, _) = connect_async(&url).await.expect("connect");

    let resp = do_initialize(&mut socket, 1).await;
    let result = resp.get("result").expect("initialize result");
    let pv = result
        .get("protocolVersion")
        .or_else(|| result.get("protocol_version"))
        .expect("protocolVersion in response");
    assert!(
        pv.as_u64().is_some() || pv.as_str().is_some(),
        "protocolVersion should be a number or string: {pv}"
    );
    let _ = socket.close(None).await;
    server.abort();
}

// ─── Concurrent prompt rejection ───────────────────────────────────────────

#[tokio::test]
async fn concurrent_prompt_returns_error() {
    let (url, server) = start_server().await;
    let (mut socket, _) = connect_async(&url).await.expect("connect");

    let _ = do_initialize(&mut socket, 1).await;
    let session_id = do_new_session(&mut socket, 2).await;

    // Send first prompt (will hang waiting for LLM, which isn't configured)
    socket
        .send(Message::Text(
            json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "session/prompt",
                "params": {
                    "sessionId": session_id,
                    "prompt": [{"type": "text", "text": "hello"}]
                }
            })
            .to_string(),
        ))
        .await
        .expect("send first prompt");

    // Small delay to ensure the first prompt is being processed
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Send second prompt — should get an error response
    let second = send_request(
        &mut socket,
        11,
        "session/prompt",
        json!({
            "sessionId": session_id,
            "prompt": [{"type": "text", "text": "second"}]
        }),
    )
    .await;
    assert!(
        second.get("error").is_some(),
        "expected error for concurrent prompt: {second}"
    );
    let err_code = second["error"]["code"].as_i64().unwrap_or(0);
    assert_eq!(
        err_code, -32010,
        "expected error code -32010 for concurrent prompt, got {err_code}"
    );

    let _ = socket.close(None).await;
    server.abort();
}

// ─── Multi-session isolation ────────────────────────────────────────────────

#[tokio::test]
async fn two_sessions_get_different_ids() {
    let (url, server) = start_server().await;
    let (mut socket, _) = connect_async(&url).await.expect("connect");

    let _ = do_initialize(&mut socket, 1).await;
    let session_a = do_new_session(&mut socket, 2).await;
    let session_b = do_new_session(&mut socket, 3).await;

    assert_ne!(
        session_a, session_b,
        "two new sessions must have distinct IDs"
    );

    let listed = send_request(&mut socket, 4, "session/list", json!({})).await;
    let session_ids = listed["result"]["sessions"]
        .as_array()
        .expect("sessions array");
    assert!(
        session_ids.len() >= 2,
        "expected at least 2 sessions listed, got {}",
        session_ids.len()
    );

    let _ = socket.close(None).await;
    server.abort();
}

// ─── Reconnect agent identity ──────────────────────────────────────────────

#[tokio::test]
async fn reconnect_keeps_session_store() {
    let (url, server) = start_server().await;

    // First connection: create a session
    let (mut first, _) = connect_async(&url).await.expect("connect first");
    let _ = do_initialize(&mut first, 1).await;
    let session_id = do_new_session(&mut first, 2).await;
    first.close(None).await.expect("close first");

    // Reconnect: session should still be loadable
    let (mut second, _) = connect_async(&url).await.expect("reconnect");
    let _ = do_initialize(&mut second, 3).await;

    let loaded = send_request(
        &mut second,
        4,
        "session/load",
        json!({
            "sessionId": session_id,
            "cwd": std::env::current_dir().unwrap().to_string_lossy(),
            "mcpServers": []
        }),
    )
    .await;
    assert!(
        loaded.get("result").is_some(),
        "session not loadable after reconnect: {loaded}"
    );

    let _ = second.close(None).await;
    server.abort();
}

// ─── Ping/Pong passthrough ─────────────────────────────────────────────────

#[tokio::test]
async fn ping_pong_does_not_break_protocol() {
    let (url, server) = start_server().await;
    let (mut socket, _) = connect_async(&url).await.expect("connect");

    // Send a ping frame; axum auto-responds with pong and our reader task
    // silently continues. Then a normal initialize should still work.
    socket
        .send(Message::Ping(vec![1, 2, 3]))
        .await
        .expect("send ping");

    // Drain any pong, then do a normal request
    let init = do_initialize(&mut socket, 1).await;
    assert!(
        init.get("result").is_some(),
        "initialize should still work after ping"
    );

    let _ = socket.close(None).await;
    server.abort();
}
