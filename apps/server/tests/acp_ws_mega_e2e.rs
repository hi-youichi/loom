//! End-to-end ACP-over-WebSocket smoke/contract test.
//!
//! Starts the real Axum router on TCP and speaks JSON-RPC through a real WS
//! client. It intentionally stops before `session/prompt`, which would need a
//! configured LLM; protocol lifecycle coverage remains fully networked.

use axum::Router;
use futures::{SinkExt, StreamExt};
use loom_server::{routes::build_router, state::new_state};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream};

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

async fn request(
    socket: &mut WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
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

#[tokio::test]
async fn acp_websocket_mega_initialize_session_and_reconnect() {
    let (url, server) = start_server().await;
    let (mut first, _) = connect_async(&url).await.expect("connect ACP WS");

    let initialize = request(
        &mut first,
        1,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientInfo": {"name":"ws-mega", "version":"0.1"},
            "capabilities": {"session": {"list": {}}}
        }),
    )
    .await;
    assert!(
        initialize.get("result").is_some(),
        "initialize response: {initialize}"
    );

    let created = request(
        &mut first,
        2,
        "session/new",
        json!({
            "cwd": std::env::current_dir().expect("cwd").to_string_lossy(),
            "mcpServers": []
        }),
    )
    .await;
    let session_id = created["result"]["sessionId"]
        .as_str()
        .expect("session id")
        .to_owned();
    first.close(None).await.expect("close first connection");

    let (mut second, _) = connect_async(&url).await.expect("reconnect ACP WS");
    let initialize_again = request(
        &mut second,
        3,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientInfo": {"name":"ws-mega", "version":"0.1"},
            "capabilities": {"session": {"list": {}}}
        }),
    )
    .await;
    assert!(initialize_again.get("result").is_some());

    // `session/load` must reuse the server-owned in-memory session entry
    // rather than create a fresh ACP session after reconnecting.
    let loaded = request(
        &mut second,
        4,
        "session/load",
        json!({
            "sessionId": session_id,
            "cwd": std::env::current_dir().expect("cwd").to_string_lossy(),
            "mcpServers": []
        }),
    )
    .await;
    assert!(
        loaded.get("result").is_some(),
        "session was not retained: {loaded}"
    );

    second.close(None).await.expect("close second connection");
    server.abort();
}
