//! Integration tests for LanceStore. Run with: cargo test -p graphweave --features lance --test memory_lance

#![cfg(feature = "lance")]

mod init_logging;

use async_trait::async_trait;
use graphweave::memory::{Embedder, LanceStore, Store};
use std::sync::Arc;

/// Mock embedder: returns a fixed vector per text (hash-based) for deterministic tests.
struct MockEmbedder {
    dimension: usize,
}

impl MockEmbedder {
    fn new(dimension: usize) -> Self {
        Self { dimension }
    }

    fn text_to_vec(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0f32; self.dimension];
        for (i, b) in text.bytes().enumerate() {
            v[i % self.dimension] += b as f32 / 256.0;
        }
        v
    }
}

#[async_trait]
impl Embedder for MockEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, graphweave::memory::StoreError> {
        Ok(texts.iter().map(|t| self.text_to_vec(t)).collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

#[tokio::test]
async fn lance_store_put_get_list() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lance-store");
    let embedder = Arc::new(MockEmbedder::new(4));
    let store = LanceStore::new(&path, embedder).await.unwrap();
    let ns = vec!["user1".into(), "memories".into()];

    store
        .put(&ns, "k1", &serde_json::json!({"text": "hello world"}))
        .await
        .unwrap();
    store
        .put(&ns, "k2", &serde_json::json!({"text": "foo bar"}))
        .await
        .unwrap();

    let v = store.get(&ns, "k1").await.unwrap();
    assert_eq!(v, Some(serde_json::json!({"text": "hello world"})));

    let keys = store.list(&ns).await.unwrap();
    assert!(keys.contains(&"k1".into()));
    assert!(keys.contains(&"k2".into()));
}

#[tokio::test]
async fn lance_store_put_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lance-store");
    let embedder = Arc::new(MockEmbedder::new(4));
    let store = LanceStore::new(&path, embedder).await.unwrap();
    let ns = vec!["user1".into(), "mem".into()];

    store
        .put(&ns, "key", &serde_json::json!({"text": "first"}))
        .await
        .unwrap();
    store
        .put(&ns, "key", &serde_json::json!({"text": "second"}))
        .await
        .unwrap();

    let v = store.get(&ns, "key").await.unwrap();
    assert_eq!(v, Some(serde_json::json!({"text": "second"})));
}

#[tokio::test]
async fn lance_store_search_with_query() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lance-store");
    let embedder = Arc::new(MockEmbedder::new(4));
    let store = LanceStore::new(&path, embedder).await.unwrap();
    let ns = vec!["user1".into(), "memories".into()];

    store
        .put(&ns, "a", &serde_json::json!({"text": "rust programming"}))
        .await
        .unwrap();
    store
        .put(&ns, "b", &serde_json::json!({"text": "python scripting"}))
        .await
        .unwrap();
    store
        .put(&ns, "c", &serde_json::json!({"text": "rust graphweave"}))
        .await
        .unwrap();

    let hits = store.search(&ns, Some("rust"), Some(10)).await.unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().any(|h| h.key == "a" || h.key == "c"));
    for h in &hits {
        assert!(h.score.is_some());
    }
}

#[tokio::test]
async fn lance_store_search_no_query() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lance-store");
    let embedder = Arc::new(MockEmbedder::new(4));
    let store = LanceStore::new(&path, embedder).await.unwrap();
    let ns = vec!["user1".into(), "mem".into()];

    store
        .put(&ns, "k1", &serde_json::json!("v1"))
        .await
        .unwrap();
    store
        .put(&ns, "k2", &serde_json::json!("v2"))
        .await
        .unwrap();

    let hits = store.search(&ns, None, Some(5)).await.unwrap();
    assert_eq!(hits.len(), 2);
    for h in &hits {
        assert!(h.score.is_none());
    }
}

#[tokio::test]
async fn lance_store_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lance-store");
    let ns = vec!["user1".into(), "mem".into()];

    {
        let embedder = Arc::new(MockEmbedder::new(4));
        let store = LanceStore::new(&path, embedder).await.unwrap();
        store
            .put(&ns, "persisted", &serde_json::json!("survives"))
            .await
            .unwrap();
    }

    let embedder = Arc::new(MockEmbedder::new(4));
    let store = LanceStore::new(&path, embedder).await.unwrap();
    let v = store.get(&ns, "persisted").await.unwrap();
    assert_eq!(v, Some(serde_json::json!("survives")));
}

#[tokio::test]
async fn lance_store_namespace_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lance-store");
    let embedder = Arc::new(MockEmbedder::new(4));
    let store = LanceStore::new(&path, embedder).await.unwrap();
    let ns1 = vec!["user1".into(), "mem".into()];
    let ns2 = vec!["user2".into(), "mem".into()];

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
}
