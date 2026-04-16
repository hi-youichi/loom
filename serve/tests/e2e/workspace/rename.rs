use super::super::common;
use futures_util::StreamExt;
use loom::{
    ClientRequest, ServerResponse, WorkspaceCreateRequest, WorkspaceListRequest,
    WorkspaceRenameRequest,
};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

#[tokio::test(flavor = "multi_thread")]
async fn e2e_workspace_rename() {
    common::load_dotenv();
    let (url, server_handle) = common::spawn_server_once().await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let create_req = ClientRequest::WorkspaceCreate(WorkspaceCreateRequest {
        id: "req-create-rename".to_string(),
        name: Some("original name".to_string()),
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &create_req)
        .await
        .unwrap();
    let workspace_id = match resp {
        ServerResponse::WorkspaceCreate(r) => r.workspace_id,
        other => panic!("expected WorkspaceCreate, got {:?}", other),
    };

    let rename_req = ClientRequest::WorkspaceRename(WorkspaceRenameRequest {
        id: "req-rename".to_string(),
        workspace_id: workspace_id.clone(),
        name: "renamed".to_string(),
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &rename_req)
        .await
        .unwrap();
    match resp {
        ServerResponse::WorkspaceRename(r) => {
            assert_eq!(r.workspace_id, workspace_id);
            assert_eq!(r.name, "renamed");
        }
        other => panic!("expected WorkspaceRename, got {:?}", other),
    }

    let list_req = ClientRequest::WorkspaceList(WorkspaceListRequest {
        id: "req-list-after-rename".to_string(),
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &list_req)
        .await
        .unwrap();
    match resp {
        ServerResponse::WorkspaceList(r) => {
            let found = r
                .workspaces
                .iter()
                .find(|w| w.id == workspace_id);
            let found = found
                .expect("renamed workspace should appear in list");
            assert_eq!(found.name.as_deref(), Some("renamed"));
        }
        other => panic!("expected WorkspaceList, got {:?}", other),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}
