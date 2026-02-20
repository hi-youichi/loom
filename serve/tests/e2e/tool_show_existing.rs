use super::common;
use futures_util::StreamExt;
use loom::{ClientRequest, ServerResponse, ToolShowRequest, ToolShowOutput};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn e2e_tool_show_existing() {
    common::load_dotenv();
    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let id = "tool-show-1".to_string();
    let req = ClientRequest::ToolShow(ToolShowRequest {
        id: id.clone(),
        name: "read".to_string(),
        output: Some(ToolShowOutput::Json),
        working_folder: None,
        thread_id: None,
    });
    let (resp, received) = common::send_and_recv(&mut write, &mut read, &req).await.unwrap();

    assert!(
        received.contains("\"type\":\"tool_show\"") && received.contains("\"name\":\"read\""),
        "expected tool_show response with read, received: {}",
        received
    );
    match &resp {
        ServerResponse::ToolShow(r) => {
            assert_eq!(r.id, id);
            assert!(r.tool.is_some(), "expected JSON tool spec");
            let tool = r.tool.as_ref().unwrap();
            assert_eq!(tool.get("name").and_then(|v| v.as_str()), Some("read"));
        }
        ServerResponse::Error(e) => panic!("server error: {}", e.error),
        _ => panic!("expected ToolShow, got {:?}", resp),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}
