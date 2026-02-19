//! In-memory vector store for semantic search.
//!
//! Uses embeddings for semantic similarity search. Not persistent.

use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value as JsonValue;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::SystemTime;

use crate::memory::embedder::Embedder;
use crate::memory::store::{
    Item, ListNamespacesOptions, MatchCondition, Namespace, NamespaceMatchType, SearchItem,
    SearchOptions, Store, StoreError, StoreOp, StoreOpResult, StoreSearchHit,
};

/// Pure in-memory vector store for semantic search.
///
/// **Interaction**: Used as `Arc<dyn Store>`; nodes use it for cross-thread
/// memory with semantic search.
///
/// **In-Memory**: All data stored in memory, lost when store is dropped.
pub struct InMemoryVectorStore {
    data: DashMap<String, VectorEntry>,
    embedder: Arc<dyn Embedder>,
}

/// Entry in the vector store.
#[derive(Clone)]
struct VectorEntry {
    vector: Vec<f32>,
    value: JsonValue,
    namespace: Namespace,
    key: String,
    created_at: SystemTime,
    updated_at: SystemTime,
}

impl VectorEntry {
    fn new(namespace: Namespace, key: String, value: JsonValue, vector: Vec<f32>) -> Self {
        let now = SystemTime::now();
        Self {
            vector,
            value,
            namespace,
            key,
            created_at: now,
            updated_at: now,
        }
    }

    fn update(&mut self, value: JsonValue, vector: Vec<f32>) {
        self.value = value;
        self.vector = vector;
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

impl InMemoryVectorStore {
    /// Creates a new in-memory vector store.
    ///
    /// # Arguments
    ///
    /// * `embedder` - Embedder for vector generation
    ///
    /// # Example
    ///
    /// ```ignore
    /// let embedder = Arc::new(OpenAIEmbedder::new("text-embedding-3-small"));
    /// let store = InMemoryVectorStore::new(embedder);
    /// ```
    pub fn new(embedder: Arc<dyn Embedder>) -> Self {
        Self {
            data: DashMap::new(),
            embedder,
        }
    }

    /// Extracts embeddable text from a JSON value.
    fn text_from_value(value: &JsonValue) -> String {
        value
            .get("text")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| value.to_string())
    }

    /// Computes cosine similarity between two vectors.
    ///
    /// Returns 0.0 if either vector has zero magnitude.
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot_product / (norm_a * norm_b)
        }
    }

    /// Creates a compound key from namespace and key.
    fn make_key(namespace: &Namespace, key: &str) -> String {
        format!(
            "{}:{}",
            serde_json::to_string(namespace).unwrap_or_default(),
            key
        )
    }

    /// Gets the namespace prefix for filtering.
    fn namespace_prefix(namespace: &Namespace) -> String {
        format!("{}:", serde_json::to_string(namespace).unwrap_or_default())
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

#[async_trait]
impl Store for InMemoryVectorStore {
    async fn put(
        &self,
        namespace: &Namespace,
        key: &str,
        value: &JsonValue,
    ) -> Result<(), StoreError> {
        let text = Self::text_from_value(value);

        let vectors = self.embedder.embed(&[&text]).await?;
        let vector = vectors
            .into_iter()
            .next()
            .ok_or_else(|| StoreError::EmbeddingError("No vector returned".into()))?;

        let compound_key = Self::make_key(namespace, key);

        if let Some(mut existing) = self.data.get_mut(&compound_key) {
            existing.update(value.clone(), vector);
        } else {
            let entry = VectorEntry::new(namespace.clone(), key.to_string(), value.clone(), vector);
            self.data.insert(compound_key, entry);
        }

        Ok(())
    }

    async fn get(&self, namespace: &Namespace, key: &str) -> Result<Option<JsonValue>, StoreError> {
        let compound_key = Self::make_key(namespace, key);

        Ok(self
            .data
            .get(&compound_key)
            .map(|entry| entry.value.clone()))
    }

    async fn get_item(&self, namespace: &Namespace, key: &str) -> Result<Option<Item>, StoreError> {
        let compound_key = Self::make_key(namespace, key);

        Ok(self.data.get(&compound_key).map(|entry| entry.to_item()))
    }

    async fn delete(&self, namespace: &Namespace, key: &str) -> Result<(), StoreError> {
        let compound_key = Self::make_key(namespace, key);
        self.data.remove(&compound_key);
        Ok(())
    }

    async fn list(&self, namespace: &Namespace) -> Result<Vec<String>, StoreError> {
        let ns_prefix = Self::namespace_prefix(namespace);

        let mut keys = Vec::new();
        for entry in self.data.iter() {
            if entry.key().starts_with(&ns_prefix) {
                keys.push(entry.value().key.clone());
            }
        }

        Ok(keys)
    }

    async fn search(
        &self,
        namespace_prefix: &Namespace,
        options: SearchOptions,
    ) -> Result<Vec<SearchItem>, StoreError> {
        let limit = options.limit.min(1000);
        let ns_prefix = Self::namespace_prefix(namespace_prefix);

        // Semantic search with query
        if let Some(ref q) = options.query {
            if !q.is_empty() {
                let vectors = self.embedder.embed(&[q]).await?;
                let query_vec = vectors
                    .into_iter()
                    .next()
                    .ok_or_else(|| StoreError::EmbeddingError("No vector returned".into()))?;

                let mut scores: Vec<(String, f32)> = Vec::new();

                for entry in self.data.iter() {
                    if entry.key().starts_with(&ns_prefix) {
                        let score = Self::cosine_similarity(&query_vec, &entry.vector);
                        scores.push((entry.key().clone(), score));
                    }
                }

                scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

                let hits: Vec<SearchItem> = scores
                    .into_iter()
                    .skip(options.offset)
                    .take(limit)
                    .filter_map(|(key, score)| {
                        self.data
                            .get(&key)
                            .map(|e| SearchItem::with_score(e.to_item(), score as f64))
                    })
                    .collect();

                return Ok(hits);
            }
        }

        // Non-semantic search (no query): return items up to limit
        let hits: Vec<SearchItem> = self
            .data
            .iter()
            .filter(|e| e.key().starts_with(&ns_prefix))
            .skip(options.offset)
            .take(limit)
            .map(|e| SearchItem::from_item(e.to_item()))
            .collect();

        Ok(hits)
    }

    async fn list_namespaces(
        &self,
        options: ListNamespacesOptions,
    ) -> Result<Vec<Namespace>, StoreError> {
        // Collect unique namespaces
        let mut namespaces: HashSet<Namespace> = self
            .data
            .iter()
            .map(|e| e.value().namespace.clone())
            .collect();

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::embedder::Embedder;
    use async_trait::async_trait;

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

    /// **Scenario**: Store can put and search entries with semantic similarity.
    #[tokio::test]
    async fn test_put_search() {
        let embedder = Arc::new(MockEmbedder::new(1536));
        let store = InMemoryVectorStore::new(embedder);

        let ns = vec!["test".into()];
        store
            .put(&ns, "key1", &serde_json::json!({"text": "hello world"}))
            .await
            .unwrap();
        store
            .put(
                &ns,
                "key2",
                &serde_json::json!({"text": "rust programming"}),
            )
            .await
            .unwrap();

        let options = SearchOptions::new().with_query("rust").with_limit(10);
        let hits = store.search(&ns, options).await.unwrap();

        assert!(!hits.is_empty());
        assert!(hits.iter().any(|h| h.item.key == "key2"));
        for hit in &hits {
            assert!(hit.score.is_some());
        }
    }

    /// **Scenario**: Store can get values by key.
    #[tokio::test]
    async fn test_get() {
        let embedder = Arc::new(MockEmbedder::new(1536));
        let store = InMemoryVectorStore::new(embedder);

        let ns = vec!["test".into()];
        store
            .put(
                &ns,
                "key1",
                &serde_json::json!({"text": "hello", "data": 123}),
            )
            .await
            .unwrap();

        let value = store.get(&ns, "key1").await.unwrap();
        assert_eq!(
            value,
            Some(serde_json::json!({"text": "hello", "data": 123}))
        );

        let not_found = store.get(&ns, "non_existent").await.unwrap();
        assert_eq!(not_found, None);
    }

    /// **Scenario**: get_item returns full Item with metadata.
    #[tokio::test]
    async fn test_get_item() {
        let embedder = Arc::new(MockEmbedder::new(1536));
        let store = InMemoryVectorStore::new(embedder);

        let ns = vec!["test".into()];
        store
            .put(&ns, "key1", &serde_json::json!({"text": "hello"}))
            .await
            .unwrap();

        let item = store.get_item(&ns, "key1").await.unwrap().unwrap();
        assert_eq!(item.namespace, ns);
        assert_eq!(item.key, "key1");
        assert!(item.created_at <= item.updated_at);
    }

    /// **Scenario**: delete removes an item.
    #[tokio::test]
    async fn test_delete() {
        let embedder = Arc::new(MockEmbedder::new(1536));
        let store = InMemoryVectorStore::new(embedder);

        let ns = vec!["test".into()];
        store
            .put(&ns, "key1", &serde_json::json!({"text": "hello"}))
            .await
            .unwrap();

        assert!(store.get(&ns, "key1").await.unwrap().is_some());

        store.delete(&ns, "key1").await.unwrap();
        assert!(store.get(&ns, "key1").await.unwrap().is_none());
    }

    /// **Scenario**: Store can list all keys in a namespace.
    #[tokio::test]
    async fn test_list() {
        let embedder = Arc::new(MockEmbedder::new(1536));
        let store = InMemoryVectorStore::new(embedder);

        let ns = vec!["test".into()];
        store
            .put(&ns, "key1", &serde_json::json!("v1"))
            .await
            .unwrap();
        store
            .put(&ns, "key2", &serde_json::json!("v2"))
            .await
            .unwrap();

        let keys = store.list(&ns).await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"key1".to_string()));
        assert!(keys.contains(&"key2".to_string()));
    }

    /// **Scenario**: Different namespaces are isolated.
    #[tokio::test]
    async fn test_namespace_isolation() {
        let embedder = Arc::new(MockEmbedder::new(1536));
        let store = InMemoryVectorStore::new(embedder);

        let ns1 = vec!["user1".into()];
        let ns2 = vec!["user2".into()];

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

    /// **Scenario**: Cosine similarity returns 0.0 for zero vectors.
    #[test]
    fn test_cosine_similarity_zero_vectors() {
        let a: Vec<f32> = vec![0.0, 0.0, 0.0];
        let b: Vec<f32> = vec![1.0, 2.0, 3.0];
        assert_eq!(InMemoryVectorStore::cosine_similarity(&a, &b), 0.0);
        assert_eq!(InMemoryVectorStore::cosine_similarity(&b, &a), 0.0);
    }

    /// **Scenario**: Cosine similarity returns 1.0 for identical vectors.
    #[test]
    fn test_cosine_similarity_identical() {
        let a: Vec<f32> = vec![1.0, 2.0, 3.0];
        let b: Vec<f32> = vec![1.0, 2.0, 3.0];
        let sim = InMemoryVectorStore::cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6, "Expected ~1.0, got {}", sim);
    }

    /// **Scenario**: Search without query returns entries up to limit.
    #[tokio::test]
    async fn test_search_no_query() {
        let embedder = Arc::new(MockEmbedder::new(1536));
        let store = InMemoryVectorStore::new(embedder);

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

    /// **Scenario**: Search with empty query returns entries up to limit.
    #[tokio::test]
    async fn test_search_empty_query() {
        let embedder = Arc::new(MockEmbedder::new(1536));
        let store = InMemoryVectorStore::new(embedder);

        let ns = vec!["test".into()];
        store
            .put(&ns, "key1", &serde_json::json!({"text": "first"}))
            .await
            .unwrap();

        let options = SearchOptions::new().with_query("").with_limit(10);
        let hits = store.search(&ns, options).await.unwrap();
        assert_eq!(hits.len(), 1);
    }

    /// **Scenario**: list_namespaces returns unique namespaces.
    #[tokio::test]
    async fn test_list_namespaces() {
        let embedder = Arc::new(MockEmbedder::new(1536));
        let store = InMemoryVectorStore::new(embedder);

        store
            .put(
                &vec!["users".into(), "u1".into()],
                "k1",
                &serde_json::json!(1),
            )
            .await
            .unwrap();
        store
            .put(
                &vec!["users".into(), "u2".into()],
                "k1",
                &serde_json::json!(2),
            )
            .await
            .unwrap();
        store
            .put(&vec!["docs".into()], "d1", &serde_json::json!(3))
            .await
            .unwrap();

        let options = ListNamespacesOptions::new();
        let namespaces = store.list_namespaces(options).await.unwrap();

        assert_eq!(namespaces.len(), 3);
    }

    /// **Scenario**: batch executes multiple operations.
    #[tokio::test]
    async fn test_batch() {
        let embedder = Arc::new(MockEmbedder::new(1536));
        let store = InMemoryVectorStore::new(embedder);
        let ns: Namespace = vec!["test".into()];

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
            StoreOpResult::Get(Some(item)) => {
                assert_eq!(item.key, "k1");
            }
            _ => panic!("expected Get result with item"),
        }
    }
}
