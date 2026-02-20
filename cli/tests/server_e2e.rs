//! Single smoke test: CLI integration with serve (run_serve_on_listener + WebSocket ping).
//! Full e2e suite lives in serve crate: `cargo test -p serve -- --nocapture`

use futures_util::{SinkExt, StreamExt};
use loom::{ClientRequest, PingRequest, ServerResponse};
use serve::run_serve_on_listener;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::test]
async fn server_e2e_smoke_ping() {
    let _ = dotenv::dotenv();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{}", addr);

    let server_handle = tokio::spawn(run_serve_on_listener(listener, true));

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let req = ClientRequest::Ping(PingRequest {
        id: "ping-smoke".to_string(),
    });
    let json = serde_json::to_string(&req).unwrap();
    write.send(Message::Text(json)).await.unwrap();
    let msg = timeout(Duration::from_secs(10), read.next())
        .await
        .unwrap()
        .expect("one message")
        .expect("ws ok");
    let text = msg.to_text().unwrap();
    let resp: ServerResponse = serde_json::from_str(text).unwrap();

    match &resp {
        ServerResponse::Pong(p) => assert_eq!(p.id, "ping-smoke"),
        _ => panic!("expected Pong, got {:?}", resp),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}
