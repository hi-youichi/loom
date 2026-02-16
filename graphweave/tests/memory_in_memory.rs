//! Unit tests for InMemoryStore (Store trait). No persistence; namespace isolation, put/get/list/search.
//! Run: cargo test -p graphweave --test memory_in_memory

mod init_logging;

use graphweave::memory::{InMemoryStore, SearchOptions, Store};
use serde_json::json;

#[tokio::test]
async fn in_memory_store_put_get_list_search() {
    let store = InMemoryStore::new();
    let ns = vec!["user1".into(), "memories".into()];

    store.put(&ns, "k1", &json!("v1")).await.unwrap();
    store.put(&ns, "k2", &json!({"x": 1})).await.unwrap();

    let v = store.get(&ns, "k1").await.unwrap();
    assert_eq!(v, Some(json!("v1")));

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
async fn in_memory_store_put_overwrite() {
    let store = InMemoryStore::new();
    let ns = vec!["u".into(), "mem".into()];
    store.put(&ns, "k", &json!("old")).await.unwrap();
    store.put(&ns, "k", &json!("new")).await.unwrap();
    let v = store.get(&ns, "k").await.unwrap();
    assert_eq!(v, Some(json!("new")));
}

#[tokio::test]
async fn in_memory_store_get_missing_returns_none() {
    let store = InMemoryStore::new();
    let ns = vec!["u".into()];
    let v = store.get(&ns, "nonexistent").await.unwrap();
    assert!(v.is_none());
}

#[tokio::test]
async fn in_memory_store_list_namespace_isolation() {
    let store = InMemoryStore::new();
    let ns1 = vec!["user1".into(), "mem".into()];
    let ns2 = vec!["user2".into(), "mem".into()];
    store.put(&ns1, "a", &json!(1)).await.unwrap();
    store.put(&ns2, "b", &json!(2)).await.unwrap();

    let keys1 = store.list(&ns1).await.unwrap();
    let keys2 = store.list(&ns2).await.unwrap();
    assert_eq!(keys1, vec!["a"]);
    assert_eq!(keys2, vec!["b"]);
}

#[tokio::test]
async fn in_memory_store_search_no_query_like_list() {
    let store = InMemoryStore::new();
    let ns = vec!["u".into()];
    store.put(&ns, "k1", &json!("a")).await.unwrap();
    store.put(&ns, "k2", &json!("b")).await.unwrap();

    let hits = store.search(&ns, SearchOptions::default()).await.unwrap();
    assert_eq!(hits.len(), 2);
    let keys: Vec<_> = hits.iter().map(|h| h.item.key.as_str()).collect();
    assert!(keys.contains(&"k1"));
    assert!(keys.contains(&"k2"));
}

#[tokio::test]
async fn in_memory_store_search_limit() {
    let store = InMemoryStore::new();
    let ns = vec!["u".into()];
    store.put(&ns, "k1", &json!("x")).await.unwrap();
    store.put(&ns, "k2", &json!("y")).await.unwrap();
    store.put(&ns, "k3", &json!("z")).await.unwrap();

    let hits = store
        .search(
            &ns,
            SearchOptions {
                limit: 2,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
    let hits_with_query = store
        .search(
            &ns,
            SearchOptions {
                query: Some("x".to_string()),
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(hits_with_query.len(), 1);
    assert_eq!(hits_with_query[0].item.key, "k1");
}
