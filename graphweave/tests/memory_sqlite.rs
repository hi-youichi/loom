//! Integration tests for SqliteSaver and SqliteStore. Run with: cargo test -p graphweave --features sqlite --test memory_sqlite

mod init_logging;

use graphweave::memory::{
    Checkpoint, CheckpointMetadata, CheckpointSource, Checkpointer, JsonSerializer, RunnableConfig,
    SearchOptions, SqliteSaver, SqliteStore, Store, CHECKPOINT_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TestState {
    value: String,
}

#[tokio::test]
async fn sqlite_saver_put_and_get_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("checkpoints.db");
    let serializer = Arc::new(JsonSerializer);
    let saver = SqliteSaver::<TestState>::new(&path, serializer).unwrap();
    let config = RunnableConfig {
        thread_id: Some("t1".into()),
        checkpoint_id: None,
        checkpoint_ns: String::new(),
        user_id: None,
        resume_from_node_id: None,
    };
    let checkpoint = Checkpoint {
        v: CHECKPOINT_VERSION,
        id: "c1".into(),
        ts: "123".into(),
        channel_values: TestState {
            value: "hello".into(),
        },
        channel_versions: HashMap::new(),
        versions_seen: HashMap::new(),
        updated_channels: None,
        pending_sends: Vec::new(),
        metadata: CheckpointMetadata {
            source: CheckpointSource::Update,
            step: 0,
            created_at: None,
            parents: HashMap::new(),
        },
    };
    let id = saver.put(&config, &checkpoint).await.unwrap();
    assert_eq!(id, "c1");

    let tuple = saver.get_tuple(&config).await.unwrap();
    let (cp, _meta) = tuple.unwrap();
    assert_eq!(cp.id, "c1");
    assert_eq!(cp.channel_values.value, "hello");
}

#[tokio::test]
async fn sqlite_saver_get_tuple_empty_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("checkpoints.db");
    let serializer = Arc::new(JsonSerializer);
    let saver = SqliteSaver::<TestState>::new(&path, serializer).unwrap();
    let config = RunnableConfig {
        thread_id: Some("t2".into()),
        checkpoint_id: None,
        checkpoint_ns: String::new(),
        user_id: None,
        resume_from_node_id: None,
    };
    let tuple = saver.get_tuple(&config).await.unwrap();
    assert!(tuple.is_none());
}

#[tokio::test]
async fn sqlite_saver_list() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("checkpoints.db");
    let serializer = Arc::new(JsonSerializer);
    let saver = SqliteSaver::<TestState>::new(&path, serializer).unwrap();
    let config = RunnableConfig {
        thread_id: Some("t3".into()),
        checkpoint_id: None,
        checkpoint_ns: "ns".into(),
        user_id: None,
        resume_from_node_id: None,
    };
    let list = saver.list(&config, None, None, None).await.unwrap();
    assert!(list.is_empty());

    let checkpoint = Checkpoint {
        v: CHECKPOINT_VERSION,
        id: "c3".into(),
        ts: "456".into(),
        channel_values: TestState {
            value: "world".into(),
        },
        channel_versions: HashMap::new(),
        versions_seen: HashMap::new(),
        updated_channels: None,
        pending_sends: Vec::new(),
        metadata: CheckpointMetadata {
            source: CheckpointSource::Input,
            step: 1,
            created_at: None,
            parents: HashMap::new(),
        },
    };
    saver.put(&config, &checkpoint).await.unwrap();
    let list = saver.list(&config, Some(10), None, None).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].checkpoint_id, "c3");
}

#[tokio::test]
async fn sqlite_store_put_get_list_search() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let store = SqliteStore::new(&path).unwrap();
    let ns = vec!["user1".into(), "memories".into()];

    store
        .put(&ns, "k1", &serde_json::json!("v1"))
        .await
        .unwrap();
    store
        .put(&ns, "k2", &serde_json::json!({"x": 1}))
        .await
        .unwrap();

    let v = store.get(&ns, "k1").await.unwrap();
    assert_eq!(v, Some(serde_json::json!("v1")));

    let keys = store.list(&ns).await.unwrap();
    assert!(keys.contains(&"k1".into()));
    assert!(keys.contains(&"k2".into()));

    let hits = store
        .search(
            &ns,
            SearchOptions {
                query: Some("v1".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].item.key, "k1");
}

#[tokio::test]
async fn sqlite_store_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let ns = vec!["user1".into(), "memories".into()];

    {
        let store = SqliteStore::new(&path).unwrap();
        store
            .put(&ns, "persisted", &serde_json::json!("survives"))
            .await
            .unwrap();
    }

    let store = SqliteStore::new(&path).unwrap();
    let v = store.get(&ns, "persisted").await.unwrap();
    assert_eq!(v, Some(serde_json::json!("survives")));
}

#[tokio::test]
async fn sqlite_store_namespace_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let ns1 = vec!["user1".into(), "mem".into()];
    let ns2 = vec!["user2".into(), "mem".into()];

    let store = SqliteStore::new(&path).unwrap();
    store
        .put(&ns1, "key", &serde_json::json!("v1"))
        .await
        .unwrap();
    store
        .put(&ns2, "key", &serde_json::json!("v2"))
        .await
        .unwrap();

    let v1 = store.get(&ns1, "key").await.unwrap();
    let v2 = store.get(&ns2, "key").await.unwrap();
    assert_eq!(v1, Some(serde_json::json!("v1")));
    assert_eq!(v2, Some(serde_json::json!("v2")));

    let keys1 = store.list(&ns1).await.unwrap();
    let keys2 = store.list(&ns2).await.unwrap();
    assert_eq!(keys1, vec!["key"]);
    assert_eq!(keys2, vec!["key"]);
}
