//! Tool source abstraction: list tools and call a tool.
//!
//! Loom routes all tool use through [`ToolSource`] rather than a concrete tool
//! registry. This keeps the ReAct runtime provider-agnostic: the think step only
//! needs a list of tool specs, and the act step only needs a way to call one by
//! name.
//!
//! ## Memory tools
//!
//! - **StoreToolSource**: long-term memory as tools (`remember`, `recall`, `search_memories`, `list_memories`).
//!   Use with `Arc<dyn Store>` and a fixed namespace; pass to `ActNode::new(Box::new(store_tools))`.
//! - **ShortTermMemoryToolSource**: one optional tool `get_recent_messages` (current conversation).
//!   Use only when you need to explicitly re-read or summarize last N messages; most flows can omit it.
//!   ActNode passes `ToolCallContext` via `call_tool_with_context` so this tool receives `state.messages`.
//! - **MemoryToolsSource**: composite of both. Use `MemoryToolsSource::new(store, namespace)` and pass to `ActNode::new(Box::new(memory_tools))` for one-line setup.
//!
//! ## Web tools
//!
//! - **WebToolsSource**: web fetching as tool (`web_fetcher`).
//!   Use `WebToolsSource::new()` to enable HTTP GET/POST capabilities; pass to `ActNode::new(Box::new(web_tools))`.
//! - **BashToolsSource**: shell command execution as tool (`bash`).
//!   Use `BashToolsSource::new()` to enable running shell commands; pass to `ActNode::new(Box::new(bash_tools))`.

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

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::state::tool_output_normalizer::{ToolOutputHint, ToolOutputStrategy};

/// Tool specification aligned with an MCP `tools/list` item.
///
/// This is the schema-facing description shown to the model during tool-aware
/// thinking. It can also be deserialized from YAML-backed tool definitions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolSpec {
    /// Tool name (e.g. used in MCP tools/call).
    pub name: String,
    /// Human-readable description for the LLM.
    pub description: Option<String>,
    /// JSON Schema for arguments (MCP inputSchema).
    pub input_schema: Value,
    /// Optional output normalization hint used by the unified tool output controller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_hint: Option<ToolOutputHint>,
}

impl ToolSpec {
    /// Attaches a tool-output normalization hint.
    pub fn with_output_hint(mut self, output_hint: ToolOutputHint) -> Self {
        self.output_hint = Some(output_hint);
        self
    }
}

impl ToolOutputHint {
    /// Creates a hint with a preferred output strategy.
    pub fn preferred(preferred_strategy: ToolOutputStrategy) -> Self {
        Self {
            preferred_strategy: Some(preferred_strategy),
            safe_inline_chars: None,
            prefer_head_tail: false,
        }
    }

    /// Sets the maximum size that is considered safe to inline directly.
    pub fn safe_inline_chars(mut self, safe_inline_chars: usize) -> Self {
        self.safe_inline_chars = Some(safe_inline_chars);
        self
    }

    /// Prefers head/tail summarization when truncation is needed.
    pub fn prefer_head_tail(mut self) -> Self {
        self.prefer_head_tail = true;
        self
    }
}

/// Result of a single tool call.
///
/// This represents the structured output returned to the ReAct runtime after a
/// tool invocation. Tools can return text or structured content like file diffs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallContent {
    /// Plain text result (most common case).
    Text(String),
    /// File modification shown as a diff.
    Diff {
        /// The file path being modified.
        path: String,
        /// The original content (None for new files).
        old_text: Option<String>,
        /// The new content after modification.
        new_text: String,
    },
    /// Terminal command output with a terminal ID.
    Terminal { terminal_id: String },
}

impl Serialize for ToolCallContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ToolCallContent::Text(t) => t.serialize(serializer),
            ToolCallContent::Diff {
                path,
                old_text,
                new_text,
            } => {
                use serde::ser::SerializeStruct;
                let mut s = serializer.serialize_struct("Diff", 3)?;
                s.serialize_field("type", "diff")?;
                s.serialize_field("path", path)?;
                s.serialize_field("old_text", old_text)?;
                s.serialize_field("new_text", new_text)?;
                s.end()
            }
            ToolCallContent::Terminal { terminal_id } => {
                use serde::ser::SerializeStruct;
                let mut s = serializer.serialize_struct("Terminal", 2)?;
                s.serialize_field("type", "terminal")?;
                s.serialize_field("terminal_id", terminal_id)?;
                s.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for ToolCallContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};

        struct ToolCallContentVisitor;

        impl<'de> Visitor<'de> for ToolCallContentVisitor {
            type Value = ToolCallContent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or a diff object")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(ToolCallContent::Text(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(ToolCallContent::Text(value))
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut path = None;
                let mut old_text = None;
                let mut new_text = None;
                let mut terminal_id = None;
                let mut content_type = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => content_type = Some(map.next_value()?),
                        "path" => path = Some(map.next_value()?),
                        "old_text" => old_text = map.next_value()?,
                        "new_text" => new_text = Some(map.next_value()?),
                        "terminal_id" => terminal_id = Some(map.next_value()?),
                        _ => {
                            let _ = map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }

                let content_type: String =
                    content_type.ok_or_else(|| de::Error::missing_field("type"))?;
                match content_type.as_str() {
                    "terminal" => Ok(ToolCallContent::Terminal {
                        terminal_id: terminal_id
                            .ok_or_else(|| de::Error::missing_field("terminal_id"))?,
                    }),
                    "diff" => Ok(ToolCallContent::Diff {
                        path: path.ok_or_else(|| de::Error::missing_field("path"))?,
                        old_text,
                        new_text: new_text.ok_or_else(|| de::Error::missing_field("new_text"))?,
                    }),
                    other => Err(de::Error::custom(format!(
                        "expected type 'diff' or 'terminal', got '{}'",
                        other
                    ))),
                }
            }
        }

        deserializer.deserialize_any(ToolCallContentVisitor)
    }
}

impl ToolCallContent {
    pub fn text(text: impl Into<String>) -> Self {
        ToolCallContent::Text(text.into())
    }

    pub fn diff(
        path: impl Into<String>,
        old_text: Option<String>,
        new_text: impl Into<String>,
    ) -> Self {
        ToolCallContent::Diff {
            path: path.into(),
            old_text,
            new_text: new_text.into(),
        }
    }

    pub fn terminal(terminal_id: impl Into<String>) -> Self {
        ToolCallContent::Terminal {
            terminal_id: terminal_id.into(),
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            ToolCallContent::Text(t) => Some(t),
            ToolCallContent::Diff { .. } | ToolCallContent::Terminal { .. } => None,
        }
    }

    pub fn into_text(self) -> String {
        match self {
            ToolCallContent::Text(t) => t,
            ToolCallContent::Diff { path, .. } => {
                format!("Modified file: {}", path)
            }
            ToolCallContent::Terminal { terminal_id } => {
                format!("Terminal: {}", terminal_id)
            }
        }
    }

    pub fn len(&self) -> usize {
        match self {
            ToolCallContent::Text(t) => t.len(),
            ToolCallContent::Diff { new_text, .. } => new_text.len(),
            ToolCallContent::Terminal { terminal_id } => terminal_id.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn to_display_string(&self) -> String {
        match self {
            ToolCallContent::Text(t) => t.clone(),
            ToolCallContent::Diff { path, .. } => {
                format!("Modified file: {}", path)
            }
            ToolCallContent::Terminal { terminal_id } => {
                format!("Terminal: {}", terminal_id)
            }
        }
    }
}

impl std::fmt::Display for ToolCallContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolCallContent::Text(t) => write!(f, "{}", t),
            ToolCallContent::Diff { path, .. } => write!(f, "Diff({})", path),
            ToolCallContent::Terminal { terminal_id } => write!(f, "Terminal({})", terminal_id),
        }
    }
}

impl From<String> for ToolCallContent {
    fn from(s: String) -> Self {
        ToolCallContent::Text(s)
    }
}

impl From<&str> for ToolCallContent {
    fn from(s: &str) -> Self {
        ToolCallContent::Text(s.to_string())
    }
}

/// Errors from listing or calling tools.
#[derive(Debug, Error)]
pub enum ToolSourceError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("invalid arguments: {0}")]
    InvalidInput(String),
    #[error("MCP/transport error: {0}")]
    Transport(String),
    #[error("JSON-RPC error: {0}")]
    JsonRpc(String),
    #[error("tool execution error: {0}")]
    ToolError(String),
}

/// Tool source contract used by ReAct runners.
///
/// [`crate::agent::react::ThinkNode`] consumes [`Self::list_tools`] to advertise
/// available tools to the model. [`crate::agent::react::ActNode`] uses
/// [`Self::call_tool`] or [`Self::call_tool_with_context`] to execute the model's
/// requested tool calls.
#[async_trait]
pub trait ToolSource: Send + Sync {
    /// Lists the tools available to the current runtime.
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError>;

    /// Calls a tool by name with JSON arguments.
    async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<ToolCallContent, ToolSourceError>;

    /// Calls a tool with optional per-step context.
    ///
    /// The default implementation ignores `ctx` and delegates to
    /// [`Self::call_tool`]. Tool sources that need access to ephemeral
    /// per-turn state, such as recent messages, can override this method.
    async fn call_tool_with_context(
        &self,
        name: &str,
        arguments: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let _ = ctx;
        self.call_tool(name, arguments).await
    }

    /// Injects per-step context before tool execution.
    ///
    /// This hook exists for implementations that prefer explicit stateful setup
    /// before one round of tool calls. The default implementation is a no-op.
    fn set_call_context(&self, _ctx: Option<ToolCallContext>) {}
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
