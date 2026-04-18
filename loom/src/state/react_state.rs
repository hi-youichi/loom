//! ReAct state and tool types for the minimal ReAct agent.
//!
//! ReActState holds messages plus per-round tool_calls and tool_results; Think/Act/Observe
//! nodes read and write these fields. ToolCall and ToolResult align with MCP `tools/call`
//! and result content.

use crate::memory::uuid6;
use crate::message::{AssistantToolCall, Message};
use crate::llm::ToolChoiceMode;
use crate::model_spec::ModelTier;
use crate::LlmUsage;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::state::tool_output_normalizer::{ToolOutputStrategy, ToolStorageRef};

/// Model configuration carried in [`ReActState`].
///
/// Supports two ways to specify a model:
/// - **Exact model**: set `model_id` to e.g. `"openai/gpt-4o"`.
/// - **Tier abstraction**: set `tier` to `Light` / `Standard` / `Strong`;
///   the `LlmProvider` resolves it to a concrete model at runtime.
///
/// Priority: `model_id` > `tier` > provider default (`None`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelConfig {
    /// Exact model identifier, e.g. `"openai/gpt-4o"`. When non-empty, takes precedence over `tier`.
    #[serde(default)]
    pub model_id: String,
    /// Tier abstraction. When `model_id` is empty, the provider resolves this to a concrete model.
    /// `ModelTier::None` means "use provider default".
    #[serde(default)]
    pub tier: ModelTier,
    /// Optional temperature override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Optional tool_choice override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoiceMode>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model_id: String::new(),
            tier: ModelTier::None,
            temperature: None,
            tool_choice: None,
        }
    }
}

/// A single tool invocation produced by the LLM (Think node) and consumed by Act.
///
/// Aligns with MCP `tools/call`: `name` and `arguments` (JSON string or object).
/// Optional `id` can be used to correlate with `ToolResult::call_id` in Observe.
///
/// **Interaction**: Written by ThinkNode from LLM output; read by ActNode to call
/// `ToolSource::call_tool(name, arguments)`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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
    /// Model configuration for the current run.
    #[serde(default)]
    pub model_config: ModelConfig,
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
    /// Flag available for downstream consumers (e.g. custom graph nodes).
    /// Defaults to `true`.
    #[serde(default)]
    pub should_continue: bool,
}

impl Default for ReActState {
    fn default() -> Self {
        Self {
            model_config: ModelConfig::default(),
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

fn normalize_tool_call_ids(mut calls: Vec<ToolCall>) -> Vec<ToolCall> {
    for tc in &mut calls {
        if tc.id.as_deref().is_none_or(|s| s.is_empty()) {
            tc.id = Some(format!("call_{}", uuid6()));
        }
    }
    calls
}

fn compute_think_usage(
    total_so_far: Option<&LlmUsage>,
    response_usage: Option<&LlmUsage>,
) -> (Option<LlmUsage>, Option<LlmUsage>) {
    match (total_so_far, response_usage) {
        (Some(t), Some(u)) => (Some(u.clone()), Some(t.accumulate(u))),
        (None, Some(u)) => (Some(u.clone()), Some(u.clone())),
        (Some(t), None) => (None, Some(t.clone())),
        (None, None) => (None, None),
    }
}

impl ReActState {
    /// Applies a Think step: append assistant message, set `tool_calls`, update usage counters.
    pub fn apply_think(
        mut self,
        content: String,
        reasoning_content: Option<String>,
        tool_calls: Vec<ToolCall>,
        response_usage: Option<LlmUsage>,
    ) -> Self {
        let (usage, total_usage) =
            compute_think_usage(self.total_usage.as_ref(), response_usage.as_ref());
        let tool_calls = normalize_tool_call_ids(tool_calls);
        let assistant_tool_calls: Vec<AssistantToolCall> = tool_calls
            .iter()
            .map(|tc| AssistantToolCall {
                id: tc.id.clone().unwrap_or_default(),
                name: tc.name.clone(),
                arguments: tc.arguments.clone(),
            })
            .collect();
        let think_message = if assistant_tool_calls.is_empty() {
            Message::assistant_with_reasoning(content, reasoning_content.clone())
        } else {
            Message::assistant_with_tool_calls_and_reasoning(
                content,
                assistant_tool_calls,
                reasoning_content.clone(),
            )
        };
        self.messages.push(think_message);
        debug!(
            message_count = self.messages.len(),
            tool_call_count = tool_calls.len(),
            tool_calls = ?tool_calls
                .iter()
                .map(|tc| format!(
                    "id={} name={} args_len={}",
                    tc.id.as_deref().unwrap_or(""),
                    tc.name,
                    tc.arguments.len()
                ))
                .collect::<Vec<_>>(),
            reasoning_len = reasoning_content.as_ref().map(|s| s.len()),
            content_len = self
                .messages
                .last()
                .and_then(|m| match m {
                    Message::Assistant(payload) => Some(payload.content.len()),
                    _ => None,
                }),
            "react_state apply_think wrote assistant message and tool_calls"
        );
        self.last_reasoning_content = reasoning_content;
        self.tool_calls = tool_calls;
        self.usage = usage;

        self.total_usage = total_usage;
        self.message_count_after_last_think = Some(self.messages.len());
        self.think_count = self.think_count.saturating_add(1);
        self
    }

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

impl crate::command::builtins::ResetState for ReActState {
    fn reset_context(&mut self) {
        let system = self
            .messages
            .iter()
            .find(|m| matches!(m, Message::System(_)))
            .cloned();
        self.messages.clear();
        if let Some(sys) = system {
            self.messages.push(sys);
        }
        self.tool_calls.clear();
        self.tool_results.clear();
        self.last_reasoning_content = None;
        self.turn_count = 0;
        self.summary = None;
        self.think_count = 0;
        self.message_count_after_last_think = None;
        self.approval_result = None;
        self.should_continue = true;
    }
}

impl crate::command::builtins::CompactState for ReActState {
    fn messages(&self) -> &[Message] {
        &self.messages
    }
    fn set_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
    }
    fn set_summary(&mut self, summary: String) {
        self.summary = Some(summary);
    }
}

impl crate::command::builtins::SummarizeState for ReActState {
    fn messages(&self) -> &[Message] {
        &self.messages
    }
    fn set_summary(&mut self, summary: String) {
        self.summary = Some(summary);
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

    #[test]
    fn apply_think_appends_message_and_increments_think_count() {
        let state = ReActState::default();
        let next = state.apply_think("hello".to_string(), None, vec![], None);
        assert_eq!(next.messages.len(), 1);
        assert_eq!(next.think_count, 1);
        assert_eq!(next.message_count_after_last_think, Some(1));
        assert!(next.tool_calls.is_empty());
    }

    #[test]
    fn apply_think_accumulates_total_usage() {
        let prior = LlmUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            prompt_tokens_details: None,
            completion_tokens_details: None,
        };
        let state = ReActState {
            total_usage: Some(prior),
            ..Default::default()
        };
        let turn = LlmUsage {
            prompt_tokens: 3,
            completion_tokens: 2,
            total_tokens: 5,
            prompt_tokens_details: None,
            completion_tokens_details: None,
        };
        let next = state.apply_think("x".into(), None, vec![], Some(turn.clone()));
        assert_eq!(next.usage.as_ref(), Some(&turn));
        let total = next.total_usage.expect("total");
        assert_eq!(total.prompt_tokens, 13);
        assert_eq!(total.completion_tokens, 7);
        assert_eq!(total.total_tokens, 20);
    }
}
