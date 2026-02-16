use graphweave::tools::{AggregateToolSource, ToolRegistry, ToolRegistryLocked, RememberTool, RecallTool, SearchMemoriesTool, ListMemoriesTool, GetRecentMessagesTool};
use graphweave::memory::{InMemoryStore, Store};
use graphweave::message::Message;
use graphweave::tool_source::ToolCallContext;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn tool_registry_registers_tools() {
    let mut registry = ToolRegistry::new();
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];

    let remember = RememberTool::new(store.clone(), ns.clone());
    let recall = RecallTool::new(store, ns);

    registry.register(Box::new(remember));
    registry.register(Box::new(recall));

    let tools = registry.list();
    assert_eq!(tools.len(), 2);
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"remember"));
    assert!(names.contains(&"recall"));
}

#[tokio::test]
async fn tool_registry_registers_multiple_tools() {
    let mut registry = ToolRegistry::new();
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];

    for i in 0..10 {
        let tool = RememberTool::new(store.clone(), ns.clone());
        registry.register(Box::new(tool));
    }

    let tools = registry.list();
    assert_eq!(tools.len(), 10);
}

#[tokio::test]
async fn tool_registry_replaces_tool_with_same_name() {
    let mut registry = ToolRegistry::new();
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];

    let tool1 = RememberTool::new(store.clone(), ns);
    let tool2 = RememberTool::new(store, ns);

    registry.register(Box::new(tool1));
    registry.register(Box::new(tool2));

    let tools = registry.list();
    assert_eq!(tools.len(), 1);
}

#[tokio::test]
async fn tool_registry_call_registered_tool() {
    let mut registry = ToolRegistry::new();
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];

    let tool = RememberTool::new(store.clone(), ns);
    registry.register(Box::new(tool));

    let args = json!({"key": "test", "value": "value"});
    let result = registry.call("remember", args, None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn tool_registry_call_unregistered_tool_returns_error() {
    let mut registry = ToolRegistry::new();
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];

    let tool = RememberTool::new(store, ns);
    registry.register(Box::new(tool));

    let args = json!({});
    let result = registry.call("nonexistent", args, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn tool_registry_locked_thread_safety() {
    let registry = ToolRegistryLocked::new();
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];

    let tool = RememberTool::new(store, ns);
    registry.register_sync(Box::new(tool));

    let tools = registry.list().await;
    assert_eq!(tools.len(), 1);
}

#[tokio::test]
async fn tool_registry_locked_concurrent_access() {
    let registry = std::sync::Arc::new(ToolRegistryLocked::new());
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];

    let tool = RememberTool::new(store, ns);
    registry.register_sync(Box::new(tool));

    let registry1 = std::sync::Arc::clone(&registry);
    let registry2 = std::sync::Arc::clone(&registry);

    let handle1 = tokio::spawn(async move {
        let tools = registry1.list().await;
        assert_eq!(tools.len(), 1);
    });

    let handle2 = tokio::spawn(async move {
        let tools = registry2.list().await;
        assert_eq!(tools.len(), 1);
    });

    handle1.await.unwrap();
    handle2.await.unwrap();
}

#[tokio::test]
async fn aggregate_tool_source_implements_tool_source() {
    let source = AggregateToolSource::new();

    let tools = source.list_tools().await;
    assert!(tools.is_ok());
    assert_eq!(tools.unwrap().len(), 0);
}

#[tokio::test]
async fn aggregate_tool_source_registers_tools() {
    let source = AggregateToolSource::new();
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];

    let remember = RememberTool::new(store.clone(), ns);
    let recall = RecallTool::new(store, ns);

    source.register_sync(Box::new(remember));
    source.register_sync(Box::new(recall));

    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 2);
}

#[tokio::test]
async fn aggregate_tool_source_call_registered_tool() {
    let source = AggregateToolSource::new();
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];

    let tool = RememberTool::new(store.clone(), ns);
    source.register_sync(Box::new(tool));

    let args = json!({"key": "k", "value": "v"});
    let result = source.call_tool("remember", args).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().text, "ok");
}

#[tokio::test]
async fn aggregate_tool_source_call_unregistered_tool_returns_error() {
    let source = AggregateToolSource::new();
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];

    let tool = RememberTool::new(store, ns);
    source.register_sync(Box::new(tool));

    let args = json!({});
    let result = source.call_tool("nonexistent", args).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn aggregate_tool_source_set_call_context() {
    let source = AggregateToolSource::new();
    let context = ToolCallContext::new(vec![Message::User("test".to_string())]);

    source.set_call_context(Some(context.clone()));
    source.set_call_context(None);

    let tool = GetRecentMessagesTool::new();
    source.register_sync(Box::new(tool));

    let result = source.call_tool_with_context("get_recent_messages", json!({}), Some(&context)).await;
    assert!(result.is_ok());

    let messages: Vec<serde_json::Value> = serde_json::from_str(&result.unwrap().text).unwrap();
    assert_eq!(messages.len(), 1);
}

#[tokio::test]
async fn aggregate_tool_source_with_multiple_tools() {
    let source = AggregateToolSource::new();
    let store = Arc::new(InMemoryStore::new());
    let ns = vec!["test".to_string()];

    let remember = RememberTool::new(store.clone(), ns.clone());
    let recall = RecallTool::new(store.clone(), ns);
    let search = SearchMemoriesTool::new(store.clone(), ns);
    let list = ListMemoriesTool::new(store, ns);

    source.register_sync(Box::new(remember));
    source.register_sync(Box::new(recall));
    source.register_sync(Box::new(search));
    source.register_sync(Box::new(list));

    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 4);

    let args1 = json!({"key": "k1", "value": "v1"});
    let result1 = source.call_tool("remember", args1).await.unwrap();
    assert_eq!(result1.text, "ok");

    let args2 = json!({"key": "k2", "value": "v2"});
    source.call_tool("remember", args2).await.unwrap();

    let args3 = json!({"key": "k1"});
    let result3 = source.call_tool("recall", args3).await.unwrap();
    assert_eq!(result3.text, "\"v1\"");

    let args4 = json!({});
    let result4 = source.call_tool("list_memories", args4).await.unwrap();
    let keys: Vec<String> = serde_json::from_str(&result4.text).unwrap();
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&"k1"));
    assert!(keys.contains(&"k2"));
}
