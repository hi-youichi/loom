//! Integration tests for loom_workspace::Store (DB creation, workspaces, thread membership).
//! Uses multi_thread runtime so Store's block_in_place is allowed.

use loom_workspace::Store;
use std::sync::Arc;
use tempfile::NamedTempFile;

fn is_uuid(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn store_new_creates_db_and_tables_reopen_same_path_works() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_path_buf();

    let store1 = Store::new(&path).unwrap();
    let id1 = store1.create_workspace(Some("ws1".into())).await.unwrap();
    assert!(is_uuid(&id1));
    drop(store1);

    let store2 = Store::new(&path).unwrap();
    let workspaces = store2.list_workspaces().await.unwrap();
    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].id, id1);
    assert_eq!(workspaces[0].name.as_deref(), Some("ws1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn create_workspace_returns_uuid_list_workspaces_includes_it() {
    let file = NamedTempFile::new().unwrap();
    let store = Store::new(file.path()).unwrap();

    let id = store.create_workspace(None).await.unwrap();
    assert!(is_uuid(&id));

    let list = store.list_workspaces().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
    assert!(list[0].name.is_none());
    assert!(list[0].created_at_ms > 0);

    let id2 = store.create_workspace(Some("my workspace".into())).await.unwrap();
    assert!(is_uuid(&id2));
    let list2 = store.list_workspaces().await.unwrap();
    assert_eq!(list2.len(), 2);
    let names: Vec<Option<&str>> = list2.iter().map(|w| w.name.as_deref()).collect();
    assert!(names.contains(&None));
    assert!(names.contains(&Some("my workspace")));
}

#[tokio::test(flavor = "multi_thread")]
async fn add_thread_to_workspace_idempotent() {
    let file = NamedTempFile::new().unwrap();
    let store = Arc::new(Store::new(file.path()).unwrap());
    let ws_id = store.create_workspace(None).await.unwrap();
    let thread_id = "thread-1";

    store
        .add_thread_to_workspace(&ws_id, thread_id)
        .await
        .unwrap();
    store
        .add_thread_to_workspace(&ws_id, thread_id)
        .await
        .unwrap();

    let threads = store.list_threads(&ws_id).await.unwrap();
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].thread_id, thread_id);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_threads_empty_then_after_add_ordered_by_created_at_desc() {
    let file = NamedTempFile::new().unwrap();
    let store = Store::new(file.path()).unwrap();
    let ws_id = store.create_workspace(None).await.unwrap();

    let empty = store.list_threads(&ws_id).await.unwrap();
    assert!(empty.is_empty());

    store.add_thread_to_workspace(&ws_id, "t1").await.unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    store.add_thread_to_workspace(&ws_id, "t2").await.unwrap();

    let threads = store.list_threads(&ws_id).await.unwrap();
    assert_eq!(threads.len(), 2);
    assert_eq!(threads[0].thread_id, "t2");
    assert_eq!(threads[1].thread_id, "t1");
    assert!(threads[0].created_at_ms >= threads[1].created_at_ms);
}

#[tokio::test(flavor = "multi_thread")]
async fn remove_thread_from_workspace_then_list_excludes_it() {
    let file = NamedTempFile::new().unwrap();
    let store = Store::new(file.path()).unwrap();
    let ws_id = store.create_workspace(None).await.unwrap();

    store.add_thread_to_workspace(&ws_id, "t1").await.unwrap();
    store.add_thread_to_workspace(&ws_id, "t2").await.unwrap();
    let before = store.list_threads(&ws_id).await.unwrap();
    assert_eq!(before.len(), 2);

    store.remove_thread_from_workspace(&ws_id, "t1").await.unwrap();
    let after = store.list_threads(&ws_id).await.unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].thread_id, "t2");

    store.remove_thread_from_workspace(&ws_id, "t2").await.unwrap();
    let empty = store.list_threads(&ws_id).await.unwrap();
    assert!(empty.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn list_threads_isolates_workspaces() {
    let file = NamedTempFile::new().unwrap();
    let store = Store::new(file.path()).unwrap();
    let ws_a = store.create_workspace(Some("A".into())).await.unwrap();
    let ws_b = store.create_workspace(Some("B".into())).await.unwrap();

    store.add_thread_to_workspace(&ws_a, "thread-a1").await.unwrap();
    store.add_thread_to_workspace(&ws_a, "thread-a2").await.unwrap();
    store.add_thread_to_workspace(&ws_b, "thread-b1").await.unwrap();

    let threads_a = store.list_threads(&ws_a).await.unwrap();
    let threads_b = store.list_threads(&ws_b).await.unwrap();

    assert_eq!(threads_a.len(), 2);
    let ids_a: Vec<&str> = threads_a.iter().map(|t| t.thread_id.as_str()).collect();
    assert!(ids_a.contains(&"thread-a1"));
    assert!(ids_a.contains(&"thread-a2"));
    assert!(!ids_a.contains(&"thread-b1"));

    assert_eq!(threads_b.len(), 1);
    assert_eq!(threads_b[0].thread_id, "thread-b1");
}
