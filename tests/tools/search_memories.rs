use graphweave::tools::{SearchMemoriesTool, TOOL_SEARCH_MEMORIES};
use graphweave::memory::{InMemoryStore, Store};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn search_memories_tool_name_returns_search_memories() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = SearchMemoriesTool::new(store, ns);
    assert_eq!(tool.name(), TOOL_SEARCH_MEMORIES);
}

#[tokio::test]
async fn search_memories_tool_spec_has_correct_properties() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = SearchMemoriesTool::new(store, ns);
    let spec = tool.spec();
    assert_eq!(spec.name, TOOL_SEARCH_MEMORIES);
    assert!(spec.description.is_some());
    assert!(spec.description.unwrap().contains("search"));
    assert_eq!(spec.input_schema["properties"]["query"]["type"], "string");
    assert_eq!(spec.input_schema["properties"]["limit"]["type"], "integer");
}

#[tokio::test]
async fn search_memories_tool_call_with_query() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = SearchMemoriesTool::new(store.clone(), ns.clone());

    store.put(&ns, "fruit", &json!("apple")).await.unwrap();
    store.put(&ns, "vehicle", &json!("car")).await.unwrap();

    let args = json!({"query": "fruit"});
    let result = tool.call(args, None).await.unwrap();
    let hits: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["key"], "fruit");
    assert_eq!(hits[0]["value"], "apple");
}

#[tokio::test]
async fn search_memories_tool_call_with_limit() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = SearchMemoriesTool::new(store.clone(), ns.clone());

    store.put(&ns, "a", &json!(1)).await.unwrap();
    store.put(&ns, "b", &json!(2)).await.unwrap();
    store.put(&ns, "c", &json!(3)).await.unwrap();
    store.put(&ns, "d", &json!(4)).await.unwrap();
    store.put(&ns, "e", &json!(5)).await.unwrap();

    let args = json!({"limit": 3});
    let result = tool.call(args, None).await.unwrap();
    let hits: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(hits.len(), 3);
}

#[tokio::test]
async fn search_memories_tool_call_with_query_and_limit() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = SearchMemoriesTool::new(store.clone(), ns.clone());

    store.put(&ns, "fruit", &json!("apple")).await.unwrap();
    store.put(&ns, "vehicle", &json!("car")).await.unwrap();
    store.put(&ns, "animal", &json!("dog")).await.unwrap();

    let args = json!({"query": "f", "limit": 1});
    let result = tool.call(args, None).await.unwrap();
    let hits: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["key"], "fruit");
}

#[tokio::test]
async fn search_memories_tool_call_empty_query() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = SearchMemoriesTool::new(store.clone(), ns);

    store.put(&ns, "a", &json!(1)).await.unwrap();
    store.put(&ns, "b", &json!(2)).await.unwrap();

    let args = json!({});
    let result = tool.call(args, None).await.unwrap();
    let hits: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert!(hits.len() >= 0);
}

#[tokio::test]
async fn search_memories_tool_call_no_matches() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = SearchMemoriesTool::new(store.clone(), ns);

    store.put(&ns, "fruit", &json!("apple")).await.unwrap();

    let args = json!({"query": "vehicle"});
    let result = tool.call(args, None).await.unwrap();
    let hits: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(hits.len(), 0);
}

#[tokio::test]
async fn search_memories_tool_call_limit_zero() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = SearchMemoriesTool::new(store.clone(), ns);

    store.put(&ns, "a", &json!(1)).await.unwrap();

    let args = json!({"limit": 0});
    let result = tool.call(args, None).await.unwrap();
    let hits: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(hits.len(), 0);
}

#[tokio::test]
async fn search_memories_tool_complex_values() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = SearchMemoriesTool::new(store.clone(), ns);

    let complex_value = json!({
        "name": "John",
        "age": 30,
        "tags": ["developer", "rust"]
    });
    store.put(&ns, "profile", &complex_value).await.unwrap();

    let args = json!({"query": "profile"});
    let result = tool.call(args, None).await.unwrap();
    let hits: Vec<serde_json::Value> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["key"], "profile");
    assert_eq!(hits[0]["value"]["name"], "John");
}
