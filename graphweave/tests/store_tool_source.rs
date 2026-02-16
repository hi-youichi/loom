//! Unit tests for StoreToolSource.
//!
//! Verifies list_tools returns 4 tools; remember → recall consistent; recall missing key
//! returns not found; list_memories / search_memories behavior.

mod init_logging;

use async_trait::async_trait;
use graphweave::memory::{Embedder, InMemoryStore, InMemoryVectorStore, Store, StoreError};
use graphweave::tool_source::{
    StoreToolSource, ToolSource, TOOL_LIST_MEMORIES, TOOL_RECALL, TOOL_REMEMBER,
    TOOL_SEARCH_MEMORIES,
};
use serde_json::json;
use std::sync::Arc;

/// Mock embedder for vector store tests.
struct MockEmbedder {
    dimension: usize,
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
async fn store_tool_source_list_tools_returns_four_tools() {
    let store: Arc<dyn Store> = Arc::new(InMemoryStore::new());
    let ns = vec!["memories".to_string()];
    let source = StoreToolSource::new(store, ns).await;
    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 4);
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&TOOL_REMEMBER));
    assert!(names.contains(&TOOL_RECALL));
    assert!(names.contains(&TOOL_SEARCH_MEMORIES));
    assert!(names.contains(&TOOL_LIST_MEMORIES));
}

#[tokio::test]
async fn store_tool_source_remember_recall_consistent() {
    let store: Arc<dyn Store> = Arc::new(InMemoryStore::new());
    let ns = vec!["memories".to_string()];
    let source = StoreToolSource::new(store, ns).await;

    let r = source
        .call_tool(
            TOOL_REMEMBER,
            json!({ "key": "pref", "value": "dark mode" }),
        )
        .await
        .unwrap();
    assert_eq!(r.text, "ok");

    let r = source
        .call_tool(TOOL_RECALL, json!({ "key": "pref" }))
        .await
        .unwrap();
    assert_eq!(r.text, "\"dark mode\"");
}

#[tokio::test]
async fn store_tool_source_recall_missing_key_returns_not_found() {
    let store: Arc<dyn Store> = Arc::new(InMemoryStore::new());
    let ns = vec!["memories".to_string()];
    let source = StoreToolSource::new(store, ns).await;

    let err = source
        .call_tool(TOOL_RECALL, json!({ "key": "nonexistent" }))
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("not found") || msg.contains("NotFound"));
}

#[tokio::test]
async fn store_tool_source_list_memories_returns_keys() {
    let store: Arc<dyn Store> = Arc::new(InMemoryStore::new());
    let ns = vec!["memories".to_string()];
    let source = StoreToolSource::new(store, ns).await;

    source
        .call_tool(TOOL_REMEMBER, json!({ "key": "a", "value": 1 }))
        .await
        .unwrap();
    source
        .call_tool(TOOL_REMEMBER, json!({ "key": "b", "value": 2 }))
        .await
        .unwrap();

    let r = source
        .call_tool(TOOL_LIST_MEMORIES, json!({}))
        .await
        .unwrap();
    let keys: Vec<String> = serde_json::from_str(&r.text).unwrap();
    assert!(keys.contains(&"a".to_string()));
    assert!(keys.contains(&"b".to_string()));
}

#[tokio::test]
async fn store_tool_source_search_memories_returns_hits() {
    let store: Arc<dyn Store> = Arc::new(InMemoryStore::new());
    let ns = vec!["memories".to_string()];
    let source = StoreToolSource::new(store, ns).await;

    source
        .call_tool(TOOL_REMEMBER, json!({ "key": "apple", "value": "fruit" }))
        .await
        .unwrap();
    source
        .call_tool(TOOL_REMEMBER, json!({ "key": "car", "value": "vehicle" }))
        .await
        .unwrap();

    let r = source
        .call_tool(
            TOOL_SEARCH_MEMORIES,
            json!({ "query": "fruit", "limit": 5 }),
        )
        .await
        .unwrap();
    let hits: Vec<serde_json::Value> = serde_json::from_str(&r.text).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].get("key").and_then(|v| v.as_str()), Some("apple"));
}

/// **Scenario**: StoreToolSource with InMemoryVectorStore + MockEmbedder.
/// Covers remember + search_memories with semantic search (embed call path).
#[tokio::test]
async fn store_tool_source_remember_search_with_vector_store() {
    let embedder = Arc::new(MockEmbedder { dimension: 8 });
    let store: Arc<dyn Store> = Arc::new(InMemoryVectorStore::new(embedder));
    let ns = vec!["memories".to_string()];
    let source = StoreToolSource::new(store, ns).await;

    source
        .call_tool(
            TOOL_REMEMBER,
            json!({ "key": "rust", "value": "programming language" }),
        )
        .await
        .unwrap();
    source
        .call_tool(
            TOOL_REMEMBER,
            json!({ "key": "python", "value": "scripting language" }),
        )
        .await
        .unwrap();

    let r = source
        .call_tool(
            TOOL_SEARCH_MEMORIES,
            json!({ "query": "programming", "limit": 5 }),
        )
        .await
        .unwrap();
    let hits: Vec<serde_json::Value> = serde_json::from_str(&r.text).unwrap();
    assert!(!hits.is_empty());
    assert!(hits
        .iter()
        .any(|h| h.get("key").and_then(|v| v.as_str()) == Some("rust")));
}
