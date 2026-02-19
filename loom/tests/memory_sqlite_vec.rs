//! Integration tests for SqliteVecStore. Run with: cargo test -p loom --test memory_sqlite_vec

mod init_logging;

use async_trait::async_trait;
use loom::memory::{
    Embedder, SearchOptions, SqliteVecStore, Store, StoreError, StoreOp, StoreOpResult,
};
use std::sync::Arc;

struct MockEmbedder {
    dimension: usize,
}

impl MockEmbedder {
    fn new(dimension: usize) -> Self {
        Self { dimension }
    }
}

#[async_trait]
impl Embedder for MockEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, StoreError> {
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = vec![0f32; self.dimension];
                for (i, b) in t.bytes().enumerate() {
                    v[i % self.dimension] += b as f32 / 256.0;
                }
                v
            })
            .collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

#[tokio::test]
async fn sqlite_vec_store_put_get_list_search() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store_vec.db");
    let embedder = Arc::new(MockEmbedder::new(8));
    let store = SqliteVecStore::new(&path, embedder).unwrap();
    let ns = vec!["user1".into(), "memories".into()];

    store
        .put(&ns, "k1", &serde_json::json!({"text": "hello world"}))
        .await
        .unwrap();
    store
        .put(&ns, "k2", &serde_json::json!({"text": "rust programming"}))
        .await
        .unwrap();

    let v = store.get(&ns, "k1").await.unwrap();
    assert_eq!(v, Some(serde_json::json!({"text": "hello world"})));

    let keys = store.list(&ns).await.unwrap();
    assert!(keys.contains(&"k1".into()));
    assert!(keys.contains(&"k2".into()));

    let options = SearchOptions::new().with_query("rust").with_limit(10);
    let hits = store.search(&ns, options).await.unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().any(|h| h.item.key == "k2"));
    for hit in &hits {
        assert!(hit.score.is_some());
    }
}

#[tokio::test]
async fn sqlite_vec_store_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store_vec.db");
    let ns = vec!["user1".into(), "memories".into()];

    {
        let embedder = Arc::new(MockEmbedder::new(8));
        let store = SqliteVecStore::new(&path, embedder).unwrap();
        store
            .put(
                &ns,
                "persisted",
                &serde_json::json!({"text": "survives restart"}),
            )
            .await
            .unwrap();
    }

    let embedder = Arc::new(MockEmbedder::new(8));
    let store = SqliteVecStore::new(&path, embedder).unwrap();
    let v = store.get(&ns, "persisted").await.unwrap();
    assert_eq!(v, Some(serde_json::json!({"text": "survives restart"})));
}

#[tokio::test]
async fn sqlite_vec_store_namespace_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store_vec.db");
    let embedder = Arc::new(MockEmbedder::new(8));
    let store = SqliteVecStore::new(&path, embedder).unwrap();
    let ns1 = vec!["user1".into(), "mem".into()];
    let ns2 = vec!["user2".into(), "mem".into()];

    store
        .put(&ns1, "key", &serde_json::json!({"text": "v1"}))
        .await
        .unwrap();
    store
        .put(&ns2, "key", &serde_json::json!({"text": "v2"}))
        .await
        .unwrap();

    let v1 = store.get(&ns1, "key").await.unwrap();
    let v2 = store.get(&ns2, "key").await.unwrap();
    assert_eq!(v1, Some(serde_json::json!({"text": "v1"})));
    assert_eq!(v2, Some(serde_json::json!({"text": "v2"})));

    let keys1 = store.list(&ns1).await.unwrap();
    let keys2 = store.list(&ns2).await.unwrap();
    assert_eq!(keys1, vec!["key"]);
    assert_eq!(keys2, vec!["key"]);
}

#[tokio::test]
async fn sqlite_vec_store_delete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store_vec.db");
    let embedder = Arc::new(MockEmbedder::new(8));
    let store = SqliteVecStore::new(&path, embedder).unwrap();
    let ns = vec!["test".into()];

    store
        .put(&ns, "key1", &serde_json::json!({"text": "hello"}))
        .await
        .unwrap();
    assert!(store.get(&ns, "key1").await.unwrap().is_some());

    store.delete(&ns, "key1").await.unwrap();
    assert!(store.get(&ns, "key1").await.unwrap().is_none());
}

#[tokio::test]
async fn sqlite_vec_store_search_no_query() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store_vec.db");
    let embedder = Arc::new(MockEmbedder::new(8));
    let store = SqliteVecStore::new(&path, embedder).unwrap();
    let ns = vec!["test".into()];

    store
        .put(&ns, "key1", &serde_json::json!({"text": "first"}))
        .await
        .unwrap();
    store
        .put(&ns, "key2", &serde_json::json!({"text": "second"}))
        .await
        .unwrap();

    let options = SearchOptions::new().with_limit(10);
    let hits = store.search(&ns, options).await.unwrap();
    assert_eq!(hits.len(), 2);
    for hit in &hits {
        assert!(hit.score.is_none());
    }
}

#[tokio::test]
async fn sqlite_vec_store_batch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store_vec.db");
    let embedder = Arc::new(MockEmbedder::new(8));
    let store = SqliteVecStore::new(&path, embedder).unwrap();
    let ns = vec!["test".into()];

    let ops = vec![
        StoreOp::Put {
            namespace: ns.clone(),
            key: "k1".into(),
            value: Some(serde_json::json!({"text": "hello"})),
        },
        StoreOp::Get {
            namespace: ns.clone(),
            key: "k1".into(),
        },
    ];

    let results = store.batch(ops).await.unwrap();
    assert_eq!(results.len(), 2);
    match &results[1] {
        StoreOpResult::Get(Some(item)) => assert_eq!(item.key, "k1"),
        _ => panic!("expected Get result with item"),
    }
}
