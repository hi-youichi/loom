use graphweave::tools::{RecallTool, TOOL_RECALL};
use graphweave::memory::{InMemoryStore, Store};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn recall_tool_name_returns_recall() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RecallTool::new(store, ns);
    assert_eq!(tool.name(), TOOL_RECALL);
}

#[tokio::test]
async fn recall_tool_spec_has_correct_properties() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RecallTool::new(store, ns);
    let spec = tool.spec();
    assert_eq!(spec.name, TOOL_RECALL);
    assert!(spec.description.is_some());
    assert!(spec.description.unwrap().contains("retrieve"));
    assert_eq!(spec.input_schema["properties"]["key"]["type"], "string");
    assert!(spec.input_schema["required"].as_array().unwrap().contains(&json!("key")));
}

#[tokio::test]
async fn recall_tool_call_retrieves_value() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RecallTool::new(store.clone(), ns);

    store.put(&ns, "pref", &json!("dark mode")).await.unwrap();

    let args = json!({"key": "pref"});
    let result = tool.call(args, None).await.unwrap();
    assert_eq!(result.text, "\"dark mode\"");
}

#[tokio::test]
async fn recall_tool_call_missing_key_returns_error() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RecallTool::new(store, ns);

    let args = json!({"key": "nonexistent"});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not found") || err.to_string().contains("NotFound"));
}

#[tokio::test]
async fn recall_tool_call_without_key_returns_error() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RecallTool::new(store, ns);

    let args = json!({});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("missing") || err.to_string().contains("InvalidInput"));
}

#[tokio::test]
async fn recall_tool_retrieve_different_value_types() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RecallTool::new(store.clone(), ns);

    store.put(&ns, "string_val", &json!("hello")).await.unwrap();
    store.put(&ns, "number_val", &json!(42)).await.unwrap();
    store.put(&ns, "bool_val", &json!(true)).await.unwrap();

    let result = tool.call(json!({"key": "string_val"}), None).await.unwrap();
    assert_eq!(result.text, "\"hello\"");

    let result = tool.call(json!({"key": "number_val"}), None).await.unwrap();
    assert_eq!(result.text, "42");

    let result = tool.call(json!({"key": "bool_val"}), None).await.unwrap();
    assert_eq!(result.text, "true");
}

#[tokio::test]
async fn recall_tool_namespace_isolation() {
    let store = Arc::new(InMemoryStore::new());
    let ns1 = vec!["user1".to_string()];
    let ns2 = vec!["user2".to_string()];

    let tool1 = RecallTool::new(store.clone(), ns1.clone());
    let tool2 = RecallTool::new(store.clone(), ns2);

    store.put(&ns1, "shared", &json!("from user1")).await.unwrap();
    store.put(&ns2, "shared", &json!("from user2")).await.unwrap();

    let result1 = tool1.call(json!({"key": "shared"}), None).await.unwrap();
    assert_eq!(result1.text, "\"from user1\"");

    let result2 = tool2.call(json!({"key": "shared"}), None).await.unwrap();
    assert_eq!(result2.text, "\"from user2\"");
}
