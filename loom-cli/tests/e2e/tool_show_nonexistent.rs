use super::common;
use futures_util::StreamExt;
use loom::{ClientRequest, ServerResponse, ToolShowRequest};
use serve::run_serve_on_listener;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn server_e2e_tool_show_nonexistent() {
    super::common::load_dotenv();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{}", addr);

    let server_handle = tokio::spawn(run_serve_on_listener(listener, true));

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let id = "tool-show-err-1".to_string();
    let req = ClientRequest::ToolShow(ToolShowRequest {
        id: id.clone(),
        name: "nonexistent_tool_xyz_123".to_string(),
        output: None,
    });
    let (resp, received) = common::send_and_recv(&mut write, &mut read, &req).await.unwrap();

    assert!(
        received.contains("\"type\":\"error\"") && received.contains("not found"),
        "expected error response, received: {}",
        received
    );
    match &resp {
        ServerResponse::Error(e) => {
            assert_eq!(e.id.as_deref(), Some(id.as_str()));
            assert!(e.error.contains("not found"), "error message: {}", e.error);
        }
        _ => panic!("expected Error, got {:?}", resp),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}
