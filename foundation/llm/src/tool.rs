//! Tool types for LLM function calling.
//!
//! This module defines the tool call, tool specification, tool source trait,
//! and output normalization types used by LLM clients and the agent runtime.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A tool call produced by the LLM (ThinkNode output, ActNode input).
///
/// Aligned with OpenAI `tool_calls` format.
/// The `id` field is optional for backward compatibility with older code.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    /// Unique identifier for correlating with ToolResult.
    /// Set by the LLM provider when emitting tool calls.
    pub id: Option<String>,
    /// Tool name as registered in ToolSource (e.g. "bash", "read_file").
    pub name: String,
    /// Arguments as JSON string; parsed in Act when calling the tool.
    pub arguments: String,
}

impl ToolCall {
    /// Creates a new tool call without an id.
    pub fn new(name: impl Into<String>, arguments: impl Into<String>) -> Self {
        Self {
            id: None,
            name: name.into(),
            arguments: arguments.into(),
        }
    }

    /// Creates a new tool call with an id.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Creates a new tool call with a generated id.
    pub fn with_generated_id(mut self) -> Self {
        self.id = Some(format!("call_{}", uuid::Uuid::new_v4()));
        self
    }

    /// Creates a ToolCall from an AssistantToolCall (OpenAI format).
    pub fn from_assistant_tool_call(tc: &crate::message::AssistantToolCall) -> Self {
        Self {
            id: Some(tc.id.clone()),
            name: tc.name.clone(),
            arguments: tc.arguments.clone(),
        }
    }
}

impl From<&crate::message::AssistantToolCall> for ToolCall {
    fn from(tc: &crate::message::AssistantToolCall) -> Self {
        Self::from_assistant_tool_call(tc)
    }
}

// ============================================================================
// Tool Specification (MCP format)
// ============================================================================

/// Tool specification aligned with an MCP `tools/list` item.
///
/// This is the schema-facing description shown to the model during tool-aware
/// thinking. It can also be deserialized from YAML-backed tool definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Creates a new tool specification.
    pub fn new(name: impl Into<String>, description: Option<String>, input_schema: Value) -> Self {
        Self {
            name: name.into(),
            description,
            input_schema,
            output_hint: None,
        }
    }

    /// Attaches a tool-output normalization hint.
    pub fn with_output_hint(mut self, output_hint: ToolOutputHint) -> Self {
        self.output_hint = Some(output_hint);
        self
    }
}

// ============================================================================
// Tool Output Normalization
// ============================================================================

/// Strategy for normalizing tool output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ToolOutputStrategy {
    /// Small result, keep inline in full.
    #[default]
    Inline,
    /// Only keep a summary, no inline content.
    SummaryOnly,
    /// Keep head and tail excerpts, suitable for logs/commands.
    HeadTail,
    /// Persist to file, return only file reference.
    FileRef,
    /// Persist to file with a small excerpt.
    FileRefWithExcerpt,
}

/// Optional metadata supplied by a tool to influence output normalization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolOutputHint {
    /// Strong preference for a specific normalization strategy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_strategy: Option<ToolOutputStrategy>,
    /// Safe inline budget for this tool when the default inline limit is too high.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_inline_chars: Option<usize>,
    /// Whether this tool generally benefits more from head/tail excerpts than summaries.
    #[serde(default)]
    pub prefer_head_tail: bool,
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
    pub fn safe_inline_chars(mut self, chars: usize) -> Self {
        self.safe_inline_chars = Some(chars);
        self
    }

    /// Prefers head/tail summarization when truncation is needed.
    pub fn prefer_head_tail(mut self) -> Self {
        self.prefer_head_tail = true;
        self
    }
}

// ============================================================================
// Tool Source Error
// ============================================================================

/// Errors from listing or calling tools.
#[derive(Debug, thiserror::Error)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_new() {
        let tc = ToolCall::new("bash", r#"{"command": "ls"}"#);
        assert_eq!(tc.name, "bash");
        assert_eq!(tc.arguments, r#"{"command": "ls"}"#);
        assert!(tc.id.is_none());
    }

    #[test]
    fn tool_call_with_id() {
        let tc = ToolCall::new("bash", "{}").with_id("call_123");
        assert_eq!(tc.id, Some("call_123".to_string()));
    }

    #[test]
    fn tool_spec_new() {
        let spec = ToolSpec::new(
            "bash",
            Some("Run a shell command".to_string()),
            serde_json::json!({"type": "object"}),
        );
        assert_eq!(spec.name, "bash");
        assert_eq!(spec.description, Some("Run a shell command".to_string()));
    }

    #[test]
    fn tool_call_serialize() {
        let tc = ToolCall::new("bash", "{}");
        let json = serde_json::to_string(&tc).unwrap();
        assert!(json.contains("bash"));
    }

    #[test]
    fn tool_call_deserialize() {
        let json = r#"{"name":"bash","arguments":"{}"}"#;
        let tc: ToolCall = serde_json::from_str(json).unwrap();
        assert_eq!(tc.name, "bash");
    }

    #[test]
    fn tool_output_hint_preferred() {
        let hint = ToolOutputHint::preferred(ToolOutputStrategy::HeadTail);
        assert_eq!(hint.preferred_strategy, Some(ToolOutputStrategy::HeadTail));
    }
}
