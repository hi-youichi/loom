use loom::tools::{ListMemoriesTool, TOOL_LIST_MEMORIES};
use loom::memory::{InMemoryStore, Store};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn list_memories_tool_name_returns_list_memories() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = ListMemoriesTool::new(store, ns);
    assert_eq!(tool.name(), TOOL_LIST_MEMORIES);
}

#[tokio::test]
async fn list_memories_tool_spec_has_correct_properties() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = ListMemoriesTool::new(store, ns);
    let spec = tool.spec();
    assert_eq!(spec.name, TOOL_LIST_MEMORIES);
    assert!(spec.description.is_some());
    assert!(spec.description.unwrap().contains("list"));
    assert_eq!(spec.input_schema["type"], "object");
}

#[tokio::test]
async fn list_memories_tool_call_empty_namespace() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = ListMemoriesTool::new(store.clone(), ns);

    let args = json!({});
    let result = tool.call(args, None).await.unwrap();
    let keys: Vec<String> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(keys.len(), 0);
}

#[tokio::test]
async fn list_memories_tool_call_with_items() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = ListMemoriesTool::new(store.clone(), ns);

    store.put(&ns, "a", &json!(1)).await.unwrap();
    store.put(&ns, "b", &json!(2)).await.unwrap();
    store.put(&ns, "c", &json!(3)).await.unwrap();

    let args = json!({});
    let result = tool.call(args, None).await.unwrap();
    let keys: Vec<String> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(keys.len(), 3);
    assert!(keys.contains(&"a".to_string()));
    assert!(keys.contains(&"b".to_string()));
    assert!(keys.contains(&"c".to_string()));
}

#[tokio::test]
async fn list_memories_tool_call_ignores_args() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = ListMemoriesTool::new(store.clone(), ns);

    store.put(&ns, "key1", &json!("value1")).await.unwrap();
    store.put(&ns, "key2", &json!("value2")).await.unwrap();

    let args = json!({"some": "unused", "param": 123});
    let result = tool.call(args, None).await.unwrap();
    let keys: Vec<String> = serde_json::from_str(&result.text).unwrap();
    assert_eq!(keys.len(), 2);
}

#[tokio::test]
async fn list_memories_tool_namespace_isolation() {
    let store = Arc::new(InMemoryStore::new());
    let ns1 = vec!["user1".to_string()];
    let ns2 = vec!["user2".to_string()];

    let tool1 = ListMemoriesTool::new(store.clone(), ns1);
    let tool2 = ListMemoriesTool::new(store.clone(), ns2);

    store.put(&ns1, "key", &json!("user1_value")).await.unwrap();
    store.put(&ns2, "key", &json!("user2_value")).await.unwrap();

    let args = json!({});
    let result1 = tool1.call(args, None).await.unwrap();
    let keys1: Vec<String> = serde_json::from_str(&result1.text).unwrap();
    assert_eq!(keys1.len(), 1);
    assert_eq!(keys1[0], "user1_value");

    let result2 = tool2.call(args, None).await.unwrap();
    let keys2: Vec<String> = serde_json::from_str(&result2.text).unwrap();
    assert_eq!(keys2.len(), 1);
    assert_eq!(keys2[0], "user2_value");
}

#[tokio::test]
async fn list_memories_tool_returns_json_array() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = ListMemoriesTool::new(store.clone(), ns);

    store.put(&ns, "key1", &json!("value1")).await.unwrap();
    store.put(&ns, "key2", &json!("value2")).await.unwrap();

    let args = json!({});
    let result = tool.call(args, None).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result.text).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 2);
}
