use super::common;
use futures_util::StreamExt;
use loom::{
    ClientRequest, ServerResponse, WorkspaceCreateRequest, WorkspaceListRequest,
    WorkspaceThreadAddRequest, WorkspaceThreadListRequest, WorkspaceThreadRemoveRequest,
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

    // Create workspace
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

    // List workspaces
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

#[tokio::test(flavor = "multi_thread")]
async fn e2e_workspace_thread_add_list_remove() {
    common::load_dotenv();
    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    // Create workspace
    let create_req = ClientRequest::WorkspaceCreate(WorkspaceCreateRequest {
        id: "wc-1".to_string(),
        name: None,
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &create_req)
        .await
        .unwrap();

    let workspace_id = match resp {
        ServerResponse::WorkspaceCreate(r) => r.workspace_id,
        other => panic!("expected WorkspaceCreate, got {:?}", other),
    };

    // Add thread
    let add_req = ClientRequest::WorkspaceThreadAdd(WorkspaceThreadAddRequest {
        id: "wta-1".to_string(),
        workspace_id: workspace_id.clone(),
        thread_id: "thread-1".to_string(),
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &add_req)
        .await
        .unwrap();
    match resp {
        ServerResponse::WorkspaceThreadAdd(r) => {
            assert_eq!(r.workspace_id, workspace_id);
            assert_eq!(r.thread_id, "thread-1");
        }
        other => panic!("expected WorkspaceThreadAdd, got {:?}", other),
    }

    // List threads
    let list_req = ClientRequest::WorkspaceThreadList(WorkspaceThreadListRequest {
        id: "wtl-1".to_string(),
        workspace_id: workspace_id.clone(),
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &list_req)
        .await
        .unwrap();
    match resp {
        ServerResponse::WorkspaceThreadList(r) => {
            assert_eq!(r.workspace_id, workspace_id);
            assert_eq!(r.threads.len(), 1);
            assert_eq!(r.threads[0].thread_id, "thread-1");
        }
        other => panic!("expected WorkspaceThreadList, got {:?}", other),
    }

    // Remove thread
    let remove_req = ClientRequest::WorkspaceThreadRemove(WorkspaceThreadRemoveRequest {
        id: "wtr-1".to_string(),
        workspace_id: workspace_id.clone(),
        thread_id: "thread-1".to_string(),
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &remove_req)
        .await
        .unwrap();
    match resp {
        ServerResponse::WorkspaceThreadRemove(r) => {
            assert_eq!(r.workspace_id, workspace_id);
            assert_eq!(r.thread_id, "thread-1");
        }
        other => panic!("expected WorkspaceThreadRemove, got {:?}", other),
    }

    // Verify thread removed
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &list_req)
        .await
        .unwrap();
    match resp {
        ServerResponse::WorkspaceThreadList(r) => {
            assert!(r.threads.is_empty());
        }
        other => panic!("expected WorkspaceThreadList, got {:?}", other),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}
