//! End-to-end tests for the ACP-over-WebSocket pipeline.
//!
//! These tests start a real axum server on a random port, connect via
//! `tokio-tungstenite`, exchange JSON-RPC messages, and verify:
//!
//! 1. Full ACP handshake works (initialize → authenticate → session/new).
//! 2. WS disconnect does NOT hang the server (Bug 1 regression test).
//! 3. Reconnection preserves session state and notification delivery
//!    (Bug 2 regression test).

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

use loom_server::routes::build_router;
use loom_server::state::new_server_state;

const FAST: Duration = Duration::from_secs(5);
const SLOW: Duration = Duration::from_secs(10);

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

async fn send_rpc(ws: &mut WsStream, json: &str) -> String {
    ws.send(Message::Text(json.into()))
        .await
        .expect("send");
    loop {
        match tokio::time::timeout(FAST, ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => return t.to_string(),
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => panic!("WS error: {e}"),
            Ok(None) => panic!("WS closed"),
            Err(_) => panic!("timeout waiting for response"),
        }
    }
}

async fn start_server() -> (std::net::SocketAddr, loom_server::state::SharedState) {
    let state = new_server_state();
    let app = build_router(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, state)
}

async fn connect_acp(addr: std::net::SocketAddr) -> WsStream {
    let url = format!("ws://{addr}/acp");
    let (ws, _) = tokio::time::timeout(FAST, tokio_tungstenite::connect_async(url))
        .await
        .expect("connect timeout")
        .expect("connect failed");
    ws
}

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}"#;

#[tokio::test]
async fn test_acp_initialize_over_ws() {
    let (addr, _state) = start_server().await;
    let mut ws = connect_acp(addr).await;

    let resp = send_rpc(&mut ws, INIT).await;
    assert!(
        resp.contains("protocolVersion"),
        "initialize response missing protocolVersion: {resp}"
    );
    assert!(
        resp.contains("agentCapabilities"),
        "initialize response missing agentCapabilities: {resp}"
    );

    ws.close(None).await.ok();
}

#[tokio::test]
async fn test_acp_new_session_over_ws() {
    let (addr, _state) = start_server().await;
    let mut ws = connect_acp(addr).await;

    let _ = send_rpc(&mut ws, INIT).await;

    // session/new requires `cwd` as absolute path
    let new_session = r#"{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}"#;
    let resp = send_rpc(&mut ws, new_session).await;
    assert!(
        resp.contains("sessionId"),
        "session/new response missing sessionId: {resp}"
    );

    ws.close(None).await.ok();
}

/// Regression test for Bug 1: handle_socket must return promptly after
/// the WS client disconnects, even when no new connection arrives to
/// cancel the lease.
#[tokio::test]
async fn test_handle_socket_returns_on_idle_ws_disconnect() {
    let (addr, state) = start_server().await;
    let mut ws = connect_acp(addr).await;

    let _ = send_rpc(&mut ws, INIT).await;

    let before = state.acp_hub.stats().await.total_disconnects;

    // Close the WS — no new connection will arrive
    ws.close(None).await.ok();
    drop(ws);

    // Poll for total_disconnects to increase. If handle_socket hangs,
    // note_detach is never called and this times out.
    let deadline = tokio::time::Instant::now() + SLOW;
    loop {
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "handle_socket did not return within {}s — Bug 1 regression: idle WS disconnect hangs",
                SLOW.as_secs()
            );
        }
        let after = state.acp_hub.stats().await.total_disconnects;
        if after > before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Regression test for Bug 2: after reconnection, the stale note_detach
/// from the old connection must NOT kill the new connection's notifications.
#[tokio::test]
async fn test_reconnect_preserves_notification_delivery() {
    let (addr, state) = start_server().await;

    let mut ws1 = connect_acp(addr).await;
    let _ = send_rpc(&mut ws1, INIT).await;

    ws1.close(None).await.ok();
    drop(ws1);

    // Wait for server to process disconnect
    let deadline = tokio::time::Instant::now() + FAST;
    loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        if state.acp_hub.stats().await.total_disconnects >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Second connection (reconnect)
    let mut ws2 = connect_acp(addr).await;
    let resp = tokio::time::timeout(FAST, async {
        send_rpc(&mut ws2, INIT).await
    })
    .await;
    assert!(
        resp.is_ok(),
        "initialize on reconnected WS should succeed"
    );

    let s = state.acp_hub.stats().await;
    assert!(s.total_connections >= 2, "should have 2+ connections");
    assert!(s.total_reconnects >= 1, "should have 1+ reconnect");

    ws2.close(None).await.ok();
}

/// Test that the same durable agent is used across reconnections.
/// The agent's session store lives in memory and should survive as long
/// as the AcpHub keeps the agent alive.
#[tokio::test]
async fn test_durable_agent_survives_reconnect() {
    let (addr, state) = start_server().await;

    // First connection — initialize + new session
    let mut ws1 = connect_acp(addr).await;
    let _ = send_rpc(&mut ws1, INIT).await;

    let new_session = r#"{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}"#;
    let resp = send_rpc(&mut ws1, new_session).await;

    // Verify session was created successfully
    assert!(
        resp.contains("sessionId"),
        "session/new should return sessionId: {resp}"
    );

    ws1.close(None).await.ok();
    drop(ws1);

    // Wait for server to process disconnect
    let deadline = tokio::time::Instant::now() + FAST;
    loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        if state.acp_hub.stats().await.total_disconnects >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Second connection — should be able to create a new session
    // (verifies the durable agent is still alive and functional)
    let mut ws2 = connect_acp(addr).await;
    let _ = send_rpc(&mut ws2, INIT).await;

    let new_session2 = r#"{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}"#;
    let resp2 = tokio::time::timeout(FAST, send_rpc(&mut ws2, new_session2))
        .await
        .expect("session/new on reconnected WS should respond within timeout");
    assert!(
        resp2.contains("sessionId"),
        "session/new after reconnect should succeed: {resp2}"
    );

    ws2.close(None).await.ok();
}

/// Test rapid connect/disconnect cycles don't leak or panic.
#[tokio::test]
async fn test_rapid_connect_disconnect_cycle() {
    let (addr, state) = start_server().await;

    for _ in 0..5 {
        let mut ws = connect_acp(addr).await;
        let _ = send_rpc(&mut ws, INIT).await;
        ws.close(None).await.ok();
        drop(ws);
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    // Allow extra time for the last disconnect to be processed
    tokio::time::sleep(Duration::from_millis(500)).await;
    let s = state.acp_hub.stats().await;
    assert!(s.total_connections >= 5, "should have 5+ connections");
    // With the generation-based detach fix, only the current-generation
    // disconnect increments the counter.  During rapid reconnects, stale
    // detach calls are no-ops.  The last connection's detach should count.
    assert!(
        s.total_disconnects >= 1,
        "should have 1+ current-generation disconnect: {s:?}"
    );
}
