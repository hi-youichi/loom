//! Unit tests for ToolRegistryLocked.
//!
//! Verifies register_sync and register_async work from async context; list/call
//! behave correctly after registration.

mod init_logging;

use async_trait::async_trait;
use loom::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use loom::tools::{Tool, ToolRegistryLocked};
use serde_json::json;

/// Mock tool for testing registry.
struct MockTool {
    name: String,
    result: String,
}

#[async_trait]
impl Tool for MockTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.clone(),
            description: None,
            input_schema: serde_json::json!({}),
        }
    }

    async fn call(
        &self,
        _args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        Ok(ToolCallContent {
            text: self.result.clone(),
        })
    }
}

/// **Scenario**: register_sync can be called from tokio async context without blocking/panic.
/// Verifies sync registration path works when invoked during async initialization.
#[tokio::test]
async fn tool_registry_register_sync_from_async_context() {
    let registry = ToolRegistryLocked::new();
    registry.register_sync(Box::new(MockTool {
        name: "mock".to_string(),
        result: "ok".to_string(),
    }));

    let tools = registry.list().await;
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "mock");

    let result = registry.call("mock", json!({}), None).await.unwrap();
    assert_eq!(result.text, "ok");
}

/// **Scenario**: register_async registers tool and list/call work correctly.
/// Verifies async registration path (preferred when in async context).
#[tokio::test]
async fn tool_registry_register_async_then_list_and_call() {
    let registry = ToolRegistryLocked::new();
    registry
        .register_async(Box::new(MockTool {
            name: "async_mock".to_string(),
            result: "async_ok".to_string(),
        }))
        .await;

    let tools = registry.list().await;
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "async_mock");

    let result = registry.call("async_mock", json!({}), None).await.unwrap();
    assert_eq!(result.text, "async_ok");
}
