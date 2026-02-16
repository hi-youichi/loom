//! Unit tests for MemoryToolsSource (composite long-term + short-term).
//!
//! Verifies list_tools returns 5 tools; call_tool dispatches to store/short-term;
//! set_call_context is forwarded so get_recent_messages sees context.

mod init_logging;

use async_trait::async_trait;
use graphweave::memory::{Embedder, InMemoryStore, InMemoryVectorStore, Store, StoreError};
use graphweave::message::Message;
use graphweave::tool_source::{
    MemoryToolsSource, ToolCallContext, ToolSource, TOOL_GET_RECENT_MESSAGES, TOOL_LIST_MEMORIES,
    TOOL_RECALL, TOOL_REMEMBER, TOOL_SEARCH_MEMORIES,
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
async fn memory_tools_source_list_tools_returns_five_tools() {
    let store: Arc<dyn Store> = Arc::new(InMemoryStore::new());
    let ns = vec!["memories".to_string()];
    let source = MemoryToolsSource::new(store, ns).await;
    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 5);
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&TOOL_REMEMBER));
    assert!(names.contains(&TOOL_RECALL));
    assert!(names.contains(&TOOL_LIST_MEMORIES));
    assert!(names.contains(&TOOL_GET_RECENT_MESSAGES));
}

#[tokio::test]
async fn memory_tools_source_call_tool_dispatches_to_store() {
    let store: Arc<dyn Store> = Arc::new(InMemoryStore::new());
    let ns = vec!["memories".to_string()];
    let source = MemoryToolsSource::new(store, ns).await;

    let r = source
        .call_tool(TOOL_REMEMBER, json!({ "key": "k", "value": "v" }))
        .await
        .unwrap();
    assert_eq!(r.text, "ok");

    let r = source
        .call_tool(TOOL_RECALL, json!({ "key": "k" }))
        .await
        .unwrap();
    assert_eq!(r.text, "\"v\"");
}

#[tokio::test]
async fn memory_tools_source_set_call_context_forwarded_get_recent_messages() {
    let store: Arc<dyn Store> = Arc::new(InMemoryStore::new());
    let ns = vec!["memories".to_string()];
    let source = MemoryToolsSource::new(store, ns).await;

    source.set_call_context(Some(ToolCallContext::new(vec![
        Message::user("hi"),
        Message::assistant("hello"),
    ])));

    let r = source
        .call_tool(TOOL_GET_RECENT_MESSAGES, json!({}))
        .await
        .unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_str(&r.text).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].get("content").and_then(|v| v.as_str()), Some("hi"));
    assert_eq!(
        arr[1].get("content").and_then(|v| v.as_str()),
        Some("hello")
    );
}

/// **Scenario**: MemoryToolsSource with InMemoryVectorStore + MockEmbedder.
/// Covers remember + search_memories with semantic search (embed call path).
#[tokio::test]
async fn memory_tools_source_with_vector_store() {
    let embedder = Arc::new(MockEmbedder { dimension: 8 });
    let store: Arc<dyn Store> = Arc::new(InMemoryVectorStore::new(embedder));
    let ns = vec!["memories".to_string()];
    let source = MemoryToolsSource::new(store, ns).await;

    source
        .call_tool(
            TOOL_REMEMBER,
            json!({ "key": "lang", "value": "rust programming" }),
        )
        .await
        .unwrap();
    source
        .call_tool(TOOL_REMEMBER, json!({ "key": "food", "value": "pizza" }))
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
        .any(|h| h.get("key").and_then(|v| v.as_str()) == Some("lang")));
}
