use super::common;
use futures_util::StreamExt;
use loom::{ClientRequest, PingRequest, ServerResponse};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn e2e_ping() {
    common::load_dotenv();
    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let req = ClientRequest::Ping(PingRequest {
        id: "ping-1".to_string(),
    });
    let (resp, received) = common::send_and_recv(&mut write, &mut read, &req).await.unwrap();

    assert!(
        received.contains("\"type\":\"pong\"") && received.contains("\"id\":\"ping-1\""),
        "expected pong response, received: {}",
        received
    );
    match &resp {
        ServerResponse::Pong(p) => assert_eq!(p.id, "ping-1"),
        _ => panic!("expected Pong, got {:?}", resp),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}
