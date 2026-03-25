//! ReAct state and tool types for the minimal ReAct agent.
//!
//! ReActState holds messages plus per-round tool_calls and tool_results; Think/Act/Observe
//! nodes read and write these fields. ToolCall and ToolResult align with MCP `tools/call`
//! and result content.

use crate::message::Message;
use crate::LlmUsage;
use serde::{Deserialize, Serialize};

use crate::state::tool_output_normalizer::{ToolOutputStrategy, ToolStorageRef};

/// A single tool invocation produced by the LLM (Think node) and consumed by Act.
///
/// Aligns with MCP `tools/call`: `name` and `arguments` (JSON string or object).
/// Optional `id` can be used to correlate with `ToolResult::call_id` in Observe.
///
/// **Interaction**: Written by ThinkNode from LLM output; read by ActNode to call
/// `ToolSource::call_tool(name, arguments)`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool name as registered in ToolSource (e.g. MCP tools/list).
    pub name: String,
    /// Arguments as JSON string; parse in Act when calling the tool.
    pub arguments: String,
    /// Optional id to match with ToolResult; useful when merging results in Observe.
    pub id: Option<String>,
}

/// Result of executing one tool call (Act node output, Observe node input).
///
/// Aligns with MCP result `content[].text`. Use `call_id` or `name` to associate
/// with the corresponding `ToolCall` when merging into state in Observe.
///
/// **Interaction**: Written by ActNode from `ToolSource::call_tool` result; read by
/// ObserveNode to append to messages or internal state and then clear.
///
/// ## Unified Output Control
///
/// The struct now supports multiple semantic views of tool output:
/// - `raw_content`: Original tool output (may be `None` for large results)
/// - `observation_text`: Text injected into next LLM turn (ObserveNode uses this)
/// - `display_text`: Text shown in stream/UI (stream events use this)
/// - `storage_ref`: Reference to persisted large output (file path, etc.)
/// - `strategy`: Normalization strategy applied (Inline, HeadTail, FileRef, etc.)
///
/// For backward compatibility, `content` field is populated with `observation_text`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolResult {
    /// Id of the tool call this result belongs to (if ToolCall had `id`).
    pub call_id: Option<String>,
    /// Tool name; alternative to call_id for matching.
    pub name: Option<String>,
    /// Result content for backward compatibility (populated with observation_text).
    /// Prefer using `observation_text()` method for new code.
    pub content: String,
    /// Whether this result represents an error (tool execution failed or user rejected).
    /// Observe uses this to include error context in the message sent to the LLM.
    #[serde(default)]
    pub is_error: bool,

    // === Unified Output Control Fields ===
    /// Original raw tool output; `None` if too large and only persisted to storage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_content: Option<String>,
    /// Text to inject into next LLM turn (ObserveNode uses this).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_text: Option<String>,
    /// Text to show in stream/UI events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_text: Option<String>,
    /// Reference to persisted storage for large outputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_ref: Option<ToolStorageRef>,
    /// Normalization strategy applied to this result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<ToolOutputStrategy>,
    /// Character count of original raw output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_chars: Option<usize>,
    /// Character count of observation text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_chars: Option<usize>,
    /// Whether the output was truncated.
    #[serde(default)]
    pub truncated: bool,
}

impl ToolResult {
    /// Returns the text to inject into the next LLM turn.
    /// Falls back to `content` for backward compatibility.
    pub fn observation(&self) -> &str {
        self.observation_text.as_deref().unwrap_or(&self.content)
    }

    /// Returns the text to display in stream/UI.
    /// Falls back to observation_text, then content.
    pub fn display(&self) -> &str {
        self.display_text
            .as_deref()
            .or(self.observation_text.as_deref())
            .unwrap_or(&self.content)
    }

    /// Returns the raw original output if available.
    pub fn raw(&self) -> Option<&str> {
        self.raw_content.as_deref()
    }

    /// Creates a simple ToolResult with just content (backward compatible).
    pub fn simple(
        call_id: Option<String>,
        name: Option<String>,
        content: String,
        is_error: bool,
    ) -> Self {
        let content_chars = content.chars().count();
        Self {
            call_id,
            name,
            content: content.clone(),
            is_error,
            raw_content: Some(content.clone()),
            observation_text: Some(content.clone()),
            display_text: Some(content.clone()),
            storage_ref: None,
            strategy: Some(ToolOutputStrategy::Inline),
            raw_chars: Some(content_chars),
            observation_chars: Some(content_chars),
            truncated: false,
        }
    }

    /// Sets the call_id field.
    pub fn with_call_id(mut self, call_id: impl Into<Option<String>>) -> Self {
        self.call_id = call_id.into();
        self
    }

    /// Sets the name field.
    pub fn with_name(mut self, name: impl Into<Option<String>>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the is_error field.
    pub fn with_is_error(mut self, is_error: bool) -> Self {
        self.is_error = is_error;
        self
    }
}

impl From<crate::state::tool_output_normalizer::NormalizedToolOutput> for ToolResult {
    fn from(normalized: crate::state::tool_output_normalizer::NormalizedToolOutput) -> Self {
        Self {
            call_id: None,
            name: None,
            content: normalized.observation_text.clone(),
            is_error: false,
            raw_content: normalized.raw_content,
            observation_text: Some(normalized.observation_text),
            display_text: Some(normalized.display_text),
            storage_ref: normalized.storage_ref,
            strategy: Some(normalized.strategy),
            raw_chars: Some(normalized.raw_chars),
            observation_chars: Some(normalized.observation_chars),
            truncated: normalized.truncated,
        }
    }
}

/// State for the minimal ReAct graph: Think → Act → Observe.
///
/// Extends conversation history (`messages`) with per-round tool data: LLM outputs
/// `tool_calls`, Act fills `tool_results`, Observe merges results and clears both.
/// Satisfies `Clone + Send + Sync + 'static` for use with `Node<ReActState>` and
/// `StateGraph<ReActState>`.
///
/// **Interaction**: Consumed and produced by ThinkNode, ActNode, ObserveNode; passed
/// through `StateGraph::invoke`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReActState {
    /// Conversation history (System, User, Assistant). Used by Think and extended by Observe.
    pub messages: Vec<Message>,
    /// Most recent reasoning/thinking content returned by the LLM, if any.
    pub last_reasoning_content: Option<String>,
    /// Current round tool calls from the LLM (Think writes, Act reads).
    pub tool_calls: Vec<ToolCall>,
    /// Current round tool execution results (Act writes, Observe reads and merges).
    pub tool_results: Vec<ToolResult>,
    /// Number of observe rounds completed; incremented in ObserveNode, used to enforce max turns.
    #[serde(default)]
    pub turn_count: u32,
    /// When set, indicates the user's approval decision for the current pending tool (approval flow).
    /// Set by the caller (e.g. Server) when resuming after an `approval_required` Interrupt.
    /// Consumed by ActNode: `Some(true)` → execute the tool; `Some(false)` → add "User rejected" result.
    #[serde(default)]
    pub approval_result: Option<bool>,
    /// Token usage for the last LLM call (Think node). Set by ThinkNode when the provider returns usage.
    #[serde(default)]
    pub usage: Option<LlmUsage>,
    /// Accumulated token usage over the whole run (sum of all Think turns). Set by ThinkNode.
    #[serde(default)]
    pub total_usage: Option<LlmUsage>,
    /// Number of messages at the time of the last Think; used for hybrid token estimate in compression.
    #[serde(default)]
    pub message_count_after_last_think: Option<usize>,
    /// Number of times ThinkNode has been executed; used to detect first think for summary generation.
    #[serde(default)]
    pub think_count: u32,
    /// Session summary generated after the first think; used for session list display.
    #[serde(default)]
    pub summary: Option<String>,
    /// Flag set by CompletionCheckNode to indicate whether the task should continue.
    /// Used by conditional routing after completion_check node.
    #[serde(default)]
    pub should_continue: bool,
}

impl Default for ReActState {
    fn default() -> Self {
        Self {
            messages: vec![],
            last_reasoning_content: None,
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: 0,
            approval_result: None,
            usage: None,
            total_usage: None,
            message_count_after_last_think: None,
            think_count: 0,
            summary: None,
            should_continue: true,
        }
    }
}

impl ReActState {
    /// Returns the content of the chronologically last Assistant message, if any.
    ///
    /// Used by callers (e.g. bot, CLI) to get the final reply without scanning `messages`.
    /// Semantics: last message in `messages` that is `Message::Assistant`; returns that turn's
    /// text `content` only (ignores embedded `tool_calls`). Empty text (e.g. tool-only turn)
    /// returns `Some("")`. Returns
    /// `None` only when there is no Assistant message at all.
    pub fn last_assistant_reply(&self) -> Option<String> {
        self.messages.iter().rev().find_map(|m| match m {
            Message::Assistant(p) => Some(p.content.clone()),
            _ => None,
        })
    }

    /// Returns the most recent reasoning/thinking content captured from the LLM.
    pub fn last_reasoning_content(&self) -> Option<String> {
        self.last_reasoning_content.clone()
    }
}

// ReActState, ToolCall, ToolResult: fields are standard types (String, Vec<Message>, Option<String>, etc.),
// so they satisfy Clone + Send + Sync + 'static required by Node<S> and StateGraph<S>.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_reasoning_content_returns_latest_value() {
        let state = ReActState {
            messages: vec![],
            last_reasoning_content: Some("step by step".to_string()),
            ..Default::default()
        };
        assert_eq!(
            state.last_reasoning_content().as_deref(),
            Some("step by step")
        );
    }
}
