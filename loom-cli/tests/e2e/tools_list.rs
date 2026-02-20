use super::common;
use futures_util::StreamExt;
use loom::{ClientRequest, ServerResponse, ToolsListRequest};
use serve::run_serve_on_listener;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn server_e2e_tools_list() {
    super::common::load_dotenv();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{}", addr);

    let server_handle = tokio::spawn(run_serve_on_listener(listener, true));

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let id = "tools-list-1".to_string();
    let req = ClientRequest::ToolsList(ToolsListRequest { id: id.clone() });
    let (resp, received) = common::send_and_recv(&mut write, &mut read, &req).await.unwrap();

    assert!(
        received.contains("\"type\":\"tools_list\"") && received.contains("\"tools\""),
        "expected tools_list response, received: {}",
        received
    );
    match &resp {
        ServerResponse::ToolsList(r) => {
            assert_eq!(r.id, id);
            assert!(r.tools.len() > 0, "expected at least one tool");
        }
        ServerResponse::Error(e) => panic!("server error: {}", e.error),
        _ => panic!("expected ToolsList, got {:?}", resp),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}
