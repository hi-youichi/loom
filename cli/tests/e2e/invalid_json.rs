use futures_util::{SinkExt, StreamExt};
use loom::ServerResponse;
use serve::run_serve_on_listener;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::test]
async fn server_e2e_invalid_json_returns_error() {
    super::common::load_dotenv();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{}", addr);

    let server_handle = tokio::spawn(run_serve_on_listener(listener, true));

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    write.send(Message::Text("not valid json".to_string())).await.unwrap();
    let read_timeout = Duration::from_secs(5);
    let opt = timeout(read_timeout, read.next()).await.unwrap();
    let msg = opt.expect("expected one response").expect("ws message");
    let text = msg.to_text().unwrap_or("");
    eprintln!("[e2e] received: {}", text);

    assert!(
        text.contains("\"type\":\"error\"") && (text.contains("parse") || text.contains("json")),
        "expected error for invalid JSON, received: {}",
        text
    );
    let resp: ServerResponse = serde_json::from_str(text).unwrap();
    match &resp {
        ServerResponse::Error(e) => {
            assert!(e.error.contains("parse") || e.error.contains("json"));
        }
        _ => panic!("expected Error for invalid JSON, got {:?}", resp),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}
