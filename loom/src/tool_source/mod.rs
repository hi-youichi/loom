//! Tool source abstraction: list tools and call a tool.
//!
//! Loom routes all tool use through [`ToolSource`] rather than a concrete tool
//! registry. This keeps the ReAct runtime provider-agnostic: the think step only
//! needs a list of tool specs, and the act step only needs a way to call one by
//! name.

mod bash_tools_source;
mod context;
mod dry_run_tool_source;
mod file_tool_source;
mod filtered_tool_source;
mod memory_tools_source;
mod mock;
mod read_only_dir_tool_source;
mod short_term_memory_tool_source;
mod store_tool_source;
mod telegram_tools_source;
mod web_tools_source;
mod yaml_specs;

mod mcp;

pub use bash_tools_source::{BashToolsSource, TOOL_BASH};
pub use context::ToolCallContext;
pub use dry_run_tool_source::DryRunToolSource;
pub use file_tool_source::{register_file_tools, FileToolSource};
pub use filtered_tool_source::FilteredToolSource;
pub use memory_tools_source::MemoryToolsSource;
pub use mock::MockToolSource;
pub use read_only_dir_tool_source::{
    register_read_only_dir_tools, ReadOnlyDirToolSource, TOOL_READ_ONLY_LIST_DIR,
    TOOL_READ_ONLY_READ_FILE,
};
pub use short_term_memory_tool_source::{ShortTermMemoryToolSource, TOOL_GET_RECENT_MESSAGES};
pub use store_tool_source::{
    StoreToolSource, TOOL_LIST_MEMORIES, TOOL_RECALL, TOOL_REMEMBER, TOOL_SEARCH_MEMORIES,
};
pub use telegram_tools_source::TelegramToolsSource;
pub use web_tools_source::{WebToolsSource, TOOL_WEB_FETCHER};
pub use yaml_specs::{load_tool_specs, YamlSpecError, YamlSpecToolSource};

pub use mcp::{McpSession, McpSessionError, McpToolSource};

// Re-export types that are now defined in loom-llm
pub use loom_llm::tool::{ToolSpec, ToolOutputHint, ToolOutputStrategy, ToolSourceError};
pub use loom_llm::message::ToolCallContent;

use async_trait::async_trait;
use serde_json::Value;

/// Tool source contract used by ReAct runners.
///
/// [`crate::agent::react::ThinkNode`] consumes [`Self::list_tools`] to advertise
/// available tools to the model. [`crate::agent::react::ActNode`] uses
/// [`Self::call_tool`] to execute the model's requested tool calls.
#[async_trait]
pub trait ToolSource: Send + Sync {
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError>;

    async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Scenario**: Display of each ToolSourceError variant contains expected keywords.
    #[test]
    fn tool_source_error_display_all_variants() {
        let s = ToolSourceError::NotFound("x".into()).to_string();
        assert!(s.to_lowercase().contains("not found"), "{}", s);
        let s = ToolSourceError::InvalidInput("bad".into()).to_string();
        assert!(s.to_lowercase().contains("invalid"), "{}", s);
        let s = ToolSourceError::Transport("net".into()).to_string();
        assert!(
            s.to_lowercase().contains("transport") || s.to_lowercase().contains("mcp"),
            "{}",
            s
        );
        let s = ToolSourceError::JsonRpc("rpc".into()).to_string();
        assert!(
            s.to_lowercase().contains("json") || s.to_lowercase().contains("rpc"),
            "{}",
            s
        );
    }

    /// **Scenario**: ToolSpec and ToolCallContent can be constructed and cloned.
    #[test]
    fn tool_spec_and_tool_call_content_construct_and_clone() {
        let spec = ToolSpec {
            name: "get_time".into(),
            description: Some("Get time".into()),
            input_schema: serde_json::json!({}),
            output_hint: None,
        };
        assert_eq!(spec.name, "get_time");
        let _ = spec.clone();

        // Test Text variant
        let content = ToolCallContent::text("12:00");
        assert_eq!(content.as_text(), Some("12:00"));
        let _ = content.clone();

        // Test Diff variant
        let diff = ToolCallContent::diff("test.rs", None, "new content");
        assert!(diff.as_text().is_none());
        let _ = diff.clone();
    }
}
