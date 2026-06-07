use super::super::common;
use futures_util::StreamExt;
use loom_protocol::{
    ClientRequest, ServerResponse, WorkspaceCreateRequest, WorkspaceFileListRequest,
    WorkspaceFileReadRequest,
};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

/// Create a temp workspace directory with known files for testing.
fn setup_workspace_dir(workspace_id: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Create subdirs
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join(".git")).unwrap(); // hidden, should be skipped

    // Create files
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();
    std::fs::write(root.join("README.md"), "# Test Project").unwrap();
    std::fs::write(root.join("src").join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(root.join("src").join("lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }").unwrap();

    // Set env so the handler uses this dir
    std::env::set_var(
        "WORKSPACE_ROOT_DIR",
        root.parent().unwrap().to_string_lossy().to_string(),
    );
    // The handler resolves: WORKSPACE_ROOT_DIR / workspace_id
    // We need the dir name to match workspace_id
    // So rename the temp dir's leaf to workspace_id
    let renamed = root.parent().unwrap().join(workspace_id);
    std::fs::rename(root, &renamed).unwrap();

    // Return a TempDir that wraps the parent so it stays alive
    // Since we renamed, we create a new TempDir concept — just leak the path
    std::mem::forget(dir);
    // We need to return something that keeps the parent alive
    tempfile::tempdir().unwrap() // dummy — the real dir persists via mem::forget above
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_workspace_file_list_and_read() {
    common::load_dotenv();
    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    // 1. Create workspace first
    let create_req = ClientRequest::WorkspaceCreate(WorkspaceCreateRequest {
        id: "wc-fl-1".to_string(),
        name: Some("file-test".to_string()),
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &create_req)
        .await
        .unwrap();

    let workspace_id = match resp {
        ServerResponse::WorkspaceCreate(r) => r.workspace_id,
        ServerResponse::Error(e) => panic!("create error: {}", e.error),
        other => panic!("expected WorkspaceCreate, got {:?}", other),
    };

    // 2. Setup file system for this workspace
    let _keep_alive = setup_workspace_dir(&workspace_id);

    // 3. List root directory
    let list_req = ClientRequest::WorkspaceFileList(WorkspaceFileListRequest {
        id: "fl-1".to_string(),
        workspace_id: workspace_id.clone(),
        path: Some(String::new()),
    });
    let (resp, raw) = common::send_and_recv(&mut write, &mut read, &list_req)
        .await
        .unwrap();

    let root_entries = match resp {
        ServerResponse::WorkspaceFileList(r) => {
            assert_eq!(r.id, "fl-1");
            assert_eq!(r.workspace_id, workspace_id);
            assert!(raw.contains("Cargo.toml"));
            assert!(raw.contains("src"));
            r.entries
        }
        ServerResponse::Error(e) => panic!("file_list error: {}", e.error),
        other => panic!("expected WorkspaceFileList, got {:?}", other),
    };

    // Verify folders come first, then files
    let first_file_idx = root_entries
        .iter()
        .position(|e| e.kind == "file")
        .unwrap_or(root_entries.len());
    for entry in &root_entries[..first_file_idx] {
        assert_eq!(entry.kind, "folder", "expected folder before files");
    }
    assert!(root_entries.iter().any(|e| e.name == "Cargo.toml"));
    assert!(root_entries.iter().any(|e| e.name == "src"));

    // Hidden files should be excluded
    assert!(!root_entries.iter().any(|e| e.name == ".git"));

    // 4. Read a file
    let read_req = ClientRequest::WorkspaceFileRead(WorkspaceFileReadRequest {
        id: "fr-1".to_string(),
        workspace_id: workspace_id.clone(),
        path: "Cargo.toml".to_string(),
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &read_req)
        .await
        .unwrap();

    match resp {
        ServerResponse::WorkspaceFileRead(r) => {
            assert_eq!(r.id, "fr-1");
            assert_eq!(r.path, "Cargo.toml");
            assert!(r.content.contains("[package]"));
        }
        ServerResponse::Error(e) => panic!("file_read error: {}", e.error),
        other => panic!("expected WorkspaceFileRead, got {:?}", other),
    }

    // 5. List subdirectory
    let list_src_req = ClientRequest::WorkspaceFileList(WorkspaceFileListRequest {
        id: "fl-2".to_string(),
        workspace_id: workspace_id.clone(),
        path: Some("src".to_string()),
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &list_src_req)
        .await
        .unwrap();

    match resp {
        ServerResponse::WorkspaceFileList(r) => {
            assert_eq!(r.id, "fl-2");
            assert!(r.entries.iter().any(|e| e.name == "main.rs"));
            assert!(r.entries.iter().any(|e| e.name == "lib.rs"));
        }
        ServerResponse::Error(e) => panic!("file_list src error: {}", e.error),
        other => panic!("expected WorkspaceFileList, got {:?}", other),
    }

    // 6. Read file from subdirectory
    let read_lib_req = ClientRequest::WorkspaceFileRead(WorkspaceFileReadRequest {
        id: "fr-2".to_string(),
        workspace_id: workspace_id.clone(),
        path: "src/lib.rs".to_string(),
    });
    let (resp, _) = common::send_and_recv(&mut write, &mut read, &read_lib_req)
        .await
        .unwrap();

    match resp {
        ServerResponse::WorkspaceFileRead(r) => {
            assert!(r.content.contains("pub fn add"));
        }
        ServerResponse::Error(e) => panic!("file_read lib error: {}", e.error),
        other => panic!("expected WorkspaceFileRead, got {:?}", other),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(2), server_handle).await;
}
