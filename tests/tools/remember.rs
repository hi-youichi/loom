use graphweave::tools::{RememberTool, TOOL_REMEMBER};
use graphweave::memory::{InMemoryStore, Store};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn remember_tool_name_returns_remember() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RememberTool::new(store, ns);
    assert_eq!(tool.name(), TOOL_REMEMBER);
}

#[tokio::test]
async fn remember_tool_spec_has_correct_properties() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RememberTool::new(store, ns);
    let spec = tool.spec();
    assert_eq!(spec.name, TOOL_REMEMBER);
    assert!(spec.description.is_some());
    assert!(spec.description.unwrap().contains("long-term memory"));
    assert_eq!(spec.input_schema["properties"]["key"]["type"], "string");
    assert!(spec.input_schema["required"].as_array().unwrap().contains(&json!("key")));
}

#[tokio::test]
async fn remember_tool_call_stores_value() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RememberTool::new(store.clone(), ns);

    let args = json!({"key": "pref", "value": "dark mode"});
    let result = tool.call(args, None).await.unwrap();
    assert_eq!(result.text, "ok");

    let retrieved = store.get(&ns, "pref").await.unwrap().unwrap();
    assert_eq!(retrieved, json!("dark mode"));
}

#[tokio::test]
async fn remember_tool_call_with_null_value() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RememberTool::new(store.clone(), ns);

    let args = json!({"key": "missing_value", "value": null});
    let result = tool.call(args, None).await.unwrap();
    assert_eq!(result.text, "ok");

    let retrieved = store.get(&ns, "missing_value").await.unwrap().unwrap();
    assert_eq!(retrieved, serde_json::Value::Null);
}

#[tokio::test]
async fn remember_tool_call_missing_key_returns_error() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RememberTool::new(store, ns);

    let args = json!({"value": "some value"});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("missing") || err.to_string().contains("InvalidInput"));
}

#[tokio::test]
async fn remember_tool_multiple_calls() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RememberTool::new(store.clone(), ns);

    tool.call(json!({"key": "k1", "value": "v1"}), None).await.unwrap();
    tool.call(json!({"key": "k2", "value": "v2"}), None).await.unwrap();
    tool.call(json!({"key": "k1", "value": "updated"}), None).await.unwrap();

    let v1 = store.get(&ns, "k1").await.unwrap().unwrap();
    assert_eq!(v1, json!("updated"));
    let v2 = store.get(&ns, "k2").await.unwrap().unwrap();
    assert_eq!(v2, json!("v2"));
}

#[tokio::test]
async fn remember_tool_with_complex_json_value() {
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];
    let tool = RememberTool::new(store.clone(), ns);

    let complex_value = json!({
        "nested": {"key": "value"},
        "array": [1, 2, 3],
        "string": "hello"
    });
    let args = json!({"key": "complex", "value": complex_value});
    let result = tool.call(args, None).await.unwrap();
    assert_eq!(result.text, "ok");

    let retrieved = store.get(&ns, "complex").await.unwrap().unwrap();
    assert_eq!(retrieved, complex_value);
}
