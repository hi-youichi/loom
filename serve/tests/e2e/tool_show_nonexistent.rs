use super::common;
use futures_util::StreamExt;
use loom::{ClientRequest, ServerResponse, ToolShowRequest};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn e2e_tool_show_nonexistent() {
    common::load_dotenv();
    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let id = "tool-show-err-1".to_string();
    let req = ClientRequest::ToolShow(ToolShowRequest {
        id: id.clone(),
        name: "nonexistent_tool_xyz_123".to_string(),
        output: None,
        working_folder: None,
        thread_id: None,
    });
    let (resp, received) = common::send_and_recv(&mut write, &mut read, &req)
        .await
        .unwrap();

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
