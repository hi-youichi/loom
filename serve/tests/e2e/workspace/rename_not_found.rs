use super::super::common;
use futures_util::StreamExt;
use loom::{ClientRequest, ServerResponse, WorkspaceRenameRequest};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

#[tokio::test(flavor = "multi_thread")]
async fn e2e_workspace_rename_not_found() {
    common::load_dotenv();
    let (url, server_handle) = common::spawn_server_once().await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let rename_req = ClientRequest::WorkspaceRename(WorkspaceRenameRequest {
        id: "req-rename-notfound".to_string(),
        workspace_id: "nonexistent-id".to_string(),
        name: "name".to_string(),
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &rename_req)
        .await
        .unwrap();
    match resp {
        ServerResponse::Error(r) => {
            assert!(r.error.contains("not found"));
        }
        other => panic!("expected Error, got {:?}", other),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}
