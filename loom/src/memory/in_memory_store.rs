//! In-memory Store. Not persistent.
//!
//! Semantic search uses in-memory vector store (see 16-memory-design §5.2.1).
//! This implementation does key/list and optional query filter only.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::memory::store::{
    Item, ListNamespacesOptions, MatchCondition, Namespace, NamespaceMatchType, SearchItem,
    SearchOptions, Store, StoreError, StoreOp, StoreOpResult, StoreSearchHit,
};

/// Stored entry with value and metadata.
#[derive(Debug, Clone)]
struct StoredItem {
    value: serde_json::Value,
    namespace: Namespace,
    key: String,
    created_at: SystemTime,
    updated_at: SystemTime,
}

impl StoredItem {
    fn new(namespace: Namespace, key: String, value: serde_json::Value) -> Self {
        let now = SystemTime::now();
        Self {
            value,
            namespace,
            key,
            created_at: now,
            updated_at: now,
        }
    }

    fn update(&mut self, value: serde_json::Value) {
        self.value = value;
        self.updated_at = SystemTime::now();
    }

    fn to_item(&self) -> Item {
        Item::with_timestamps(
            self.namespace.clone(),
            self.key.clone(),
            self.value.clone(),
            self.created_at,
            self.updated_at,
        )
    }
}

/// Key for the inner map: namespace joined by "\0", then key. Enables list by namespace prefix.
fn map_key(namespace: &Namespace, key: &str) -> String {
    let ns = namespace.join("\0");
    format!("{}\0{}", ns, key)
}

/// In-memory Store. Not persistent.
///
/// **Interaction**: Used as `Arc<dyn Store>` when graph is compiled with store;
/// nodes use it for cross-thread memory.
///
/// ## Example
///
/// ```rust,ignore
/// use loom::memory::{InMemoryStore, Store};
///
/// let store = InMemoryStore::new();
/// store.put(&vec!["user".into()], "key1", &json!({"data": 1})).await?;
/// ```
pub struct InMemoryStore {
    inner: Arc<RwLock<HashMap<String, StoredItem>>>,
}

impl InMemoryStore {
    /// Creates a new in-memory store.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn namespace_prefix(namespace: &Namespace) -> String {
        if namespace.is_empty() {
            String::new()
        } else {
            format!("{}\0", namespace.join("\0"))
        }
    }

    /// Checks if a namespace matches a condition.
    fn matches_condition(namespace: &Namespace, condition: &MatchCondition) -> bool {
        let path = &condition.path;

        match condition.match_type {
            NamespaceMatchType::Prefix => {
                if namespace.len() < path.len() {
                    return false;
                }
                for (i, p) in path.iter().enumerate() {
                    if p != "*" && namespace.get(i) != Some(p) {
                        return false;
                    }
                }
                true
            }
            NamespaceMatchType::Suffix => {
                if namespace.len() < path.len() {
                    return false;
                }
                let start = namespace.len() - path.len();
                for (i, p) in path.iter().enumerate() {
                    if p != "*" && namespace.get(start + i) != Some(p) {
                        return false;
                    }
                }
                true
            }
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Store for InMemoryStore {
    async fn put(
        &self,
        namespace: &Namespace,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), StoreError> {
        let k = map_key(namespace, key);
        let mut guard = self.inner.write().await;
        if let Some(existing) = guard.get_mut(&k) {
            existing.update(value.clone());
        } else {
            let item = StoredItem::new(namespace.clone(), key.to_string(), value.clone());
            guard.insert(k, item);
        }
        Ok(())
    }

    async fn get(
        &self,
        namespace: &Namespace,
        key: &str,
    ) -> Result<Option<serde_json::Value>, StoreError> {
        let k = map_key(namespace, key);
        Ok(self.inner.read().await.get(&k).map(|s| s.value.clone()))
    }

    async fn get_item(&self, namespace: &Namespace, key: &str) -> Result<Option<Item>, StoreError> {
        let k = map_key(namespace, key);
        Ok(self.inner.read().await.get(&k).map(|s| s.to_item()))
    }

    async fn delete(&self, namespace: &Namespace, key: &str) -> Result<(), StoreError> {
        let k = map_key(namespace, key);
        self.inner.write().await.remove(&k);
        Ok(())
    }

    async fn list(&self, namespace: &Namespace) -> Result<Vec<String>, StoreError> {
        let prefix = Self::namespace_prefix(namespace);
        let guard = self.inner.read().await;
        let mut keys: Vec<String> = guard
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(_, item)| item.key.clone())
            .collect();
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    async fn search(
        &self,
        namespace_prefix: &Namespace,
        options: SearchOptions,
    ) -> Result<Vec<SearchItem>, StoreError> {
        let prefix = Self::namespace_prefix(namespace_prefix);
        let guard = self.inner.read().await;

        let mut hits: Vec<SearchItem> = guard
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(_, stored)| SearchItem::from_item(stored.to_item()))
            .collect();

        // Apply query filter if provided
        if let Some(ref q) = options.query {
            if !q.is_empty() {
                hits.retain(|h| {
                    h.item.key.contains(q)
                        || h.item
                            .value
                            .to_string()
                            .to_lowercase()
                            .contains(&q.to_lowercase())
                });
            }
        }

        // Apply filter operators if provided
        if let Some(ref filter) = options.filter {
            for (field, op) in filter {
                hits.retain(|h| {
                    let field_value = h.item.value.get(field);
                    match (field_value, op) {
                        (Some(v), crate::memory::store::FilterOp::Eq(expected)) => v == expected,
                        (Some(v), crate::memory::store::FilterOp::Ne(expected)) => v != expected,
                        (Some(v), crate::memory::store::FilterOp::Gt(expected)) => {
                            compare_json(v, expected) == Some(std::cmp::Ordering::Greater)
                        }
                        (Some(v), crate::memory::store::FilterOp::Gte(expected)) => {
                            matches!(
                                compare_json(v, expected),
                                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                            )
                        }
                        (Some(v), crate::memory::store::FilterOp::Lt(expected)) => {
                            compare_json(v, expected) == Some(std::cmp::Ordering::Less)
                        }
                        (Some(v), crate::memory::store::FilterOp::Lte(expected)) => {
                            matches!(
                                compare_json(v, expected),
                                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                            )
                        }
                        _ => false,
                    }
                });
            }
        }

        // Apply offset and limit
        let offset = options.offset;
        let limit = options.limit;
        if offset > 0 {
            if offset >= hits.len() {
                hits.clear();
            } else {
                hits = hits.into_iter().skip(offset).collect();
            }
        }
        hits.truncate(limit);

        Ok(hits)
    }

    async fn list_namespaces(
        &self,
        options: ListNamespacesOptions,
    ) -> Result<Vec<Namespace>, StoreError> {
        let guard = self.inner.read().await;

        // Collect unique namespaces
        let mut namespaces: HashSet<Namespace> =
            guard.values().map(|item| item.namespace.clone()).collect();

        // Apply match conditions
        if !options.match_conditions.is_empty() {
            namespaces.retain(|ns| {
                options
                    .match_conditions
                    .iter()
                    .all(|cond| Self::matches_condition(ns, cond))
            });
        }

        // Apply max_depth: truncate namespaces to max_depth
        let mut result: Vec<Namespace> = if let Some(max_depth) = options.max_depth {
            namespaces
                .into_iter()
                .map(|ns| {
                    if ns.len() > max_depth {
                        ns.into_iter().take(max_depth).collect()
                    } else {
                        ns
                    }
                })
                .collect::<HashSet<_>>()
                .into_iter()
                .collect()
        } else {
            namespaces.into_iter().collect()
        };

        // Sort for deterministic output
        result.sort();

        // Apply offset and limit
        if options.offset > 0 {
            if options.offset >= result.len() {
                result.clear();
            } else {
                result = result.into_iter().skip(options.offset).collect();
            }
        }
        result.truncate(options.limit);

        Ok(result)
    }

    async fn batch(&self, ops: Vec<StoreOp>) -> Result<Vec<StoreOpResult>, StoreError> {
        let mut results = Vec::with_capacity(ops.len());

        for op in ops {
            let result = match op {
                StoreOp::Get { namespace, key } => {
                    let item = self.get_item(&namespace, &key).await?;
                    StoreOpResult::Get(item)
                }
                StoreOp::Put {
                    namespace,
                    key,
                    value,
                } => {
                    if let Some(v) = value {
                        self.put(&namespace, &key, &v).await?;
                    } else {
                        self.delete(&namespace, &key).await?;
                    }
                    StoreOpResult::Put
                }
                StoreOp::Search {
                    namespace_prefix,
                    options,
                } => {
                    let items = self.search(&namespace_prefix, options).await?;
                    StoreOpResult::Search(items)
                }
                StoreOp::ListNamespaces { options } => {
                    let ns = self.list_namespaces(options).await?;
                    StoreOpResult::ListNamespaces(ns)
                }
            };
            results.push(result);
        }

        Ok(results)
    }

    async fn search_simple(
        &self,
        namespace: &Namespace,
        query: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<StoreSearchHit>, StoreError> {
        let options = SearchOptions {
            query: query.map(String::from),
            filter: None,
            limit: limit.unwrap_or(10),
            offset: 0,
        };
        let results = self.search(namespace, options).await?;
        Ok(results
            .into_iter()
            .map(|si| StoreSearchHit {
                key: si.item.key,
                value: si.item.value,
                score: si.score,
            })
            .collect())
    }
}

/// Compare two JSON values for ordering (only works for numbers and strings).
fn compare_json(a: &serde_json::Value, b: &serde_json::Value) -> Option<std::cmp::Ordering> {
    use serde_json::Value;
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            let a = a.as_f64()?;
            let b = b.as_f64()?;
            a.partial_cmp(&b)
        }
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// **Scenario**: Put and get a value returns the stored value.
    #[tokio::test]
    async fn put_and_get_returns_value() {
        let store = InMemoryStore::new();
        let ns: Namespace = vec!["users".into()];
        let value = json!({"name": "Alice"});

        store.put(&ns, "u1", &value).await.unwrap();
        let result = store.get(&ns, "u1").await.unwrap();

        assert_eq!(result, Some(value));
    }

    /// **Scenario**: Get a non-existent key returns None.
    #[tokio::test]
    async fn get_nonexistent_returns_none() {
        let store = InMemoryStore::new();
        let ns: Namespace = vec!["users".into()];

        let result = store.get(&ns, "nonexistent").await.unwrap();

        assert!(result.is_none());
    }

    /// **Scenario**: get_item returns full Item with metadata.
    #[tokio::test]
    async fn get_item_returns_full_item() {
        let store = InMemoryStore::new();
        let ns: Namespace = vec!["docs".into(), "user1".into()];
        let value = json!({"content": "Hello"});

        store.put(&ns, "doc1", &value).await.unwrap();
        let item = store.get_item(&ns, "doc1").await.unwrap().unwrap();

        assert_eq!(item.namespace, ns);
        assert_eq!(item.key, "doc1");
        assert_eq!(item.value, value);
        assert!(item.created_at <= item.updated_at);
    }

    /// **Scenario**: Delete removes an item.
    #[tokio::test]
    async fn delete_removes_item() {
        let store = InMemoryStore::new();
        let ns: Namespace = vec!["test".into()];

        store.put(&ns, "k1", &json!({"x": 1})).await.unwrap();
        assert!(store.get(&ns, "k1").await.unwrap().is_some());

        store.delete(&ns, "k1").await.unwrap();
        assert!(store.get(&ns, "k1").await.unwrap().is_none());
    }

    /// **Scenario**: Delete non-existent key is idempotent (no error).
    #[tokio::test]
    async fn delete_nonexistent_is_idempotent() {
        let store = InMemoryStore::new();
        let ns: Namespace = vec!["test".into()];

        let result = store.delete(&ns, "nonexistent").await;
        assert!(result.is_ok());
    }

    /// **Scenario**: List returns all keys in namespace.
    #[tokio::test]
    async fn list_returns_all_keys() {
        let store = InMemoryStore::new();
        let ns: Namespace = vec!["items".into()];

        store.put(&ns, "a", &json!(1)).await.unwrap();
        store.put(&ns, "b", &json!(2)).await.unwrap();
        store.put(&ns, "c", &json!(3)).await.unwrap();

        let keys = store.list(&ns).await.unwrap();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    /// **Scenario**: Search with query filters results.
    #[tokio::test]
    async fn search_with_query_filters() {
        let store = InMemoryStore::new();
        let ns: Namespace = vec!["docs".into()];

        store
            .put(&ns, "doc1", &json!({"text": "hello world"}))
            .await
            .unwrap();
        store
            .put(&ns, "doc2", &json!({"text": "goodbye"}))
            .await
            .unwrap();
        store
            .put(&ns, "doc3", &json!({"text": "hello there"}))
            .await
            .unwrap();

        let options = SearchOptions::new().with_query("hello").with_limit(10);
        let results = store.search(&ns, options).await.unwrap();

        assert_eq!(results.len(), 2);
        let keys: Vec<&str> = results.iter().map(|r| r.item.key.as_str()).collect();
        assert!(keys.contains(&"doc1"));
        assert!(keys.contains(&"doc3"));
    }

    /// **Scenario**: Search with limit truncates results.
    #[tokio::test]
    async fn search_with_limit() {
        let store = InMemoryStore::new();
        let ns: Namespace = vec!["items".into()];

        for i in 0..10 {
            store
                .put(&ns, &format!("item{}", i), &json!({"i": i}))
                .await
                .unwrap();
        }

        let options = SearchOptions::new().with_limit(3);
        let results = store.search(&ns, options).await.unwrap();

        assert_eq!(results.len(), 3);
    }

    /// **Scenario**: Search with offset skips results.
    #[tokio::test]
    async fn search_with_offset() {
        let store = InMemoryStore::new();
        let ns: Namespace = vec!["items".into()];

        store.put(&ns, "a", &json!(1)).await.unwrap();
        store.put(&ns, "b", &json!(2)).await.unwrap();
        store.put(&ns, "c", &json!(3)).await.unwrap();

        let options = SearchOptions::new().with_offset(1).with_limit(10);
        let results = store.search(&ns, options).await.unwrap();

        assert_eq!(results.len(), 2);
    }

    /// **Scenario**: list_namespaces returns unique namespaces.
    #[tokio::test]
    async fn list_namespaces_returns_unique() {
        let store = InMemoryStore::new();

        store
            .put(&vec!["users".into(), "u1".into()], "k1", &json!(1))
            .await
            .unwrap();
        store
            .put(&vec!["users".into(), "u1".into()], "k2", &json!(2))
            .await
            .unwrap();
        store
            .put(&vec!["users".into(), "u2".into()], "k1", &json!(3))
            .await
            .unwrap();
        store
            .put(&vec!["docs".into()], "d1", &json!(4))
            .await
            .unwrap();

        let options = ListNamespacesOptions::new();
        let namespaces = store.list_namespaces(options).await.unwrap();

        assert_eq!(namespaces.len(), 3);
        assert!(namespaces.contains(&vec!["users".into(), "u1".into()]));
        assert!(namespaces.contains(&vec!["users".into(), "u2".into()]));
        assert!(namespaces.contains(&vec!["docs".into()]));
    }

    /// **Scenario**: list_namespaces with prefix filter.
    #[tokio::test]
    async fn list_namespaces_with_prefix() {
        let store = InMemoryStore::new();

        store
            .put(&vec!["users".into(), "u1".into()], "k1", &json!(1))
            .await
            .unwrap();
        store
            .put(&vec!["users".into(), "u2".into()], "k1", &json!(2))
            .await
            .unwrap();
        store
            .put(&vec!["docs".into()], "d1", &json!(3))
            .await
            .unwrap();

        let options = ListNamespacesOptions::new().with_prefix(vec!["users".into()]);
        let namespaces = store.list_namespaces(options).await.unwrap();

        assert_eq!(namespaces.len(), 2);
        assert!(namespaces
            .iter()
            .all(|ns| ns.first() == Some(&"users".to_string())));
    }

    /// **Scenario**: list_namespaces with max_depth truncates.
    #[tokio::test]
    async fn list_namespaces_with_max_depth() {
        let store = InMemoryStore::new();

        store
            .put(&vec!["a".into(), "b".into(), "c".into()], "k1", &json!(1))
            .await
            .unwrap();
        store
            .put(&vec!["a".into(), "b".into(), "d".into()], "k2", &json!(2))
            .await
            .unwrap();
        store
            .put(&vec!["a".into(), "x".into()], "k3", &json!(3))
            .await
            .unwrap();

        let options = ListNamespacesOptions::new().with_max_depth(2);
        let namespaces = store.list_namespaces(options).await.unwrap();

        // All namespaces truncated to depth 2: ["a", "b"], ["a", "x"]
        assert_eq!(namespaces.len(), 2);
        assert!(namespaces.iter().all(|ns| ns.len() <= 2));
    }

    /// **Scenario**: batch executes multiple operations.
    #[tokio::test]
    async fn batch_executes_multiple_ops() {
        let store = InMemoryStore::new();
        let ns: Namespace = vec!["test".into()];

        let ops = vec![
            StoreOp::Put {
                namespace: ns.clone(),
                key: "k1".into(),
                value: Some(json!({"v": 1})),
            },
            StoreOp::Put {
                namespace: ns.clone(),
                key: "k2".into(),
                value: Some(json!({"v": 2})),
            },
            StoreOp::Get {
                namespace: ns.clone(),
                key: "k1".into(),
            },
            StoreOp::Search {
                namespace_prefix: ns.clone(),
                options: SearchOptions::new(),
            },
        ];

        let results = store.batch(ops).await.unwrap();

        assert_eq!(results.len(), 4);
        match &results[0] {
            StoreOpResult::Put => {}
            _ => panic!("expected Put result"),
        }
        match &results[2] {
            StoreOpResult::Get(Some(item)) => {
                assert_eq!(item.value.get("v").and_then(|v| v.as_i64()), Some(1));
            }
            _ => panic!("expected Get result with item"),
        }
        match &results[3] {
            StoreOpResult::Search(items) => {
                assert_eq!(items.len(), 2);
            }
            _ => panic!("expected Search result"),
        }
    }

    /// **Scenario**: batch with delete (value=None) removes item.
    #[tokio::test]
    async fn batch_delete_removes_item() {
        let store = InMemoryStore::new();
        let ns: Namespace = vec!["test".into()];

        store.put(&ns, "k1", &json!(1)).await.unwrap();

        let ops = vec![
            StoreOp::Put {
                namespace: ns.clone(),
                key: "k1".into(),
                value: None, // Delete
            },
            StoreOp::Get {
                namespace: ns.clone(),
                key: "k1".into(),
            },
        ];

        let results = store.batch(ops).await.unwrap();

        match &results[1] {
            StoreOpResult::Get(None) => {}
            _ => panic!("expected Get None after delete"),
        }
    }

    /// **Scenario**: Update existing item updates timestamp.
    #[tokio::test]
    async fn update_updates_timestamp() {
        let store = InMemoryStore::new();
        let ns: Namespace = vec!["test".into()];

        store.put(&ns, "k1", &json!({"v": 1})).await.unwrap();
        let item1 = store.get_item(&ns, "k1").await.unwrap().unwrap();

        // Small delay to ensure timestamp difference
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        store.put(&ns, "k1", &json!({"v": 2})).await.unwrap();
        let item2 = store.get_item(&ns, "k1").await.unwrap().unwrap();

        assert_eq!(item1.created_at, item2.created_at);
        assert!(item2.updated_at >= item1.updated_at);
        assert_eq!(item2.value.get("v").and_then(|v| v.as_i64()), Some(2));
    }
}
