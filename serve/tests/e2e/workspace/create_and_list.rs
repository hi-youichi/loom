use super::super::common;
use futures_util::StreamExt;
use loom::{
    ClientRequest, ServerResponse, WorkspaceCreateRequest, WorkspaceListRequest,
};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

#[tokio::test(flavor = "multi_thread")]
async fn e2e_workspace_create_and_list() {
    common::load_dotenv();
    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let create_req = ClientRequest::WorkspaceCreate(WorkspaceCreateRequest {
        id: "wc-1".to_string(),
        name: Some("test-project".to_string()),
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &create_req)
        .await
        .unwrap();

    let workspace_id = match resp {
        ServerResponse::WorkspaceCreate(r) => {
            assert_eq!(r.id, "wc-1");
            assert!(!r.workspace_id.is_empty());
            r.workspace_id
        }
        ServerResponse::Error(e) => panic!("server error: {}", e.error),
        other => panic!("expected WorkspaceCreate, got {:?}", other),
    };

    let list_req = ClientRequest::WorkspaceList(WorkspaceListRequest {
        id: "wl-1".to_string(),
    });
    let (resp, received) = common::send_and_recv(&mut write, &mut read, &list_req)
        .await
        .unwrap();

    assert!(
        received.contains("\"type\":\"workspace_list\""),
        "expected workspace_list response, received: {}",
        received
    );
    match resp {
        ServerResponse::WorkspaceList(r) => {
            assert_eq!(r.id, "wl-1");
            assert!(r
                .workspaces
                .iter()
                .any(|w| w.id == workspace_id && w.name.as_deref() == Some("test-project")));
        }
        ServerResponse::Error(e) => panic!("server error: {}", e.error),
        other => panic!("expected WorkspaceList, got {:?}", other),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}
