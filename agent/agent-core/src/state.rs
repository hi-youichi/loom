//! ReAct state and tool types.

use serde::{Deserialize, Serialize};
use loom_llm::ToolChoiceMode;
use loom_llm::LlmUsage;
use loom_llm::message::{AssistantToolCall, Message};
use model_spec_core::ModelTier;
use loom_llm::support::uuid6::uuid6;

// Re-export ToolCall and ToolOutputStrategy from loom-llm
pub use loom_llm::ToolCall;
pub use loom_llm::tool::ToolOutputStrategy;

/// Reference to a persisted tool output file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStorageRef {
    pub path: std::path::PathBuf,
    pub size: usize,
    pub content_type: String,
    pub encoding: String,
    pub tool_name: String,
}

/// Model configuration carried in ReActState.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelConfig {
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub tier: ModelTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoiceMode>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self { model_id: String::new(), tier: ModelTier::None, temperature: None, tool_choice: None }
    }
}

/// Result of executing one tool call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolResult {
    pub call_id: Option<String>,
    pub name: Option<String>,
    pub content: String,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_ref: Option<ToolStorageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<ToolOutputStrategy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_chars: Option<usize>,
    #[serde(default)]
    pub truncated: bool,
}

impl ToolResult {
    pub fn observation(&self) -> &str { self.observation_text.as_deref().unwrap_or(&self.content) }
    pub fn display(&self) -> &str { self.display_text.as_deref().or(self.observation_text.as_deref()).unwrap_or(&self.content) }
    pub fn raw(&self) -> Option<&str> { self.raw_content.as_deref() }
    pub fn simple(call_id: Option<String>, name: Option<String>, content: String, is_error: bool) -> Self {
        let n = content.chars().count();
        Self { call_id, name, content: content.clone(), is_error, raw_content: Some(content.clone()), observation_text: Some(content.clone()), display_text: Some(content), storage_ref: None, strategy: Some(ToolOutputStrategy::Inline), raw_chars: Some(n), observation_chars: Some(n), truncated: false }
    }
    pub fn with_call_id(mut self, call_id: impl Into<Option<String>>) -> Self { self.call_id = call_id.into(); self }
    pub fn with_name(mut self, name: impl Into<Option<String>>) -> Self { self.name = name.into(); self }
    pub fn with_is_error(mut self, is_error: bool) -> Self { self.is_error = is_error; self }
}

/// State for the ReAct graph: Think -> Act -> Observe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReActState {
    pub model_config: ModelConfig,
    pub messages: Vec<Message>,
    pub last_reasoning_content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResult>,
    #[serde(default)]
    pub turn_count: u32,

    #[serde(default)]
    pub usage: Option<LlmUsage>,
    #[serde(default)]
    pub total_usage: Option<LlmUsage>,
    #[serde(default)]
    pub message_count_after_last_think: Option<usize>,
    #[serde(default)]
    pub think_count: u32,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default = "default_true")]
    pub should_continue: bool,
    #[serde(default)]
    pub force_compact: bool,
}

fn default_true() -> bool { true }

impl Default for ReActState {
    fn default() -> Self {
        Self {
            model_config: ModelConfig::default(), messages: vec![], last_reasoning_content: None,
            tool_calls: vec![], tool_results: vec![], turn_count: 0,
            usage: None, total_usage: None, message_count_after_last_think: None,
            think_count: 0, summary: None, should_continue: true, force_compact: false,
        }
    }
}

impl ReActState {
    pub fn apply_think(mut self, content: String, reasoning_content: Option<String>, tool_calls: Vec<ToolCall>, response_usage: Option<LlmUsage>) -> Self {
        let (usage, total_usage) = compute_think_usage(self.total_usage.as_ref(), response_usage.as_ref());
        let fixed_tool_calls: Vec<ToolCall> = tool_calls.iter().map(|tc| {
            if tc.id.as_deref().is_none_or(|s| s.is_empty()) {
                ToolCall { id: Some(format!("call_{}", uuid6())), ..tc.clone() }
            } else {
                tc.clone()
            }
        }).collect();
        let atc: Vec<AssistantToolCall> = fixed_tool_calls.iter().map(|tc| {
            AssistantToolCall { id: tc.id.clone().unwrap(), name: tc.name.clone(), arguments: tc.arguments.clone() }
        }).collect();
        let is_empty = content.trim().is_empty() && reasoning_content.as_ref().is_none_or(|s| s.trim().is_empty()) && tool_calls.is_empty();
        let msg = if atc.is_empty() { Message::assistant_with_reasoning(content, reasoning_content.clone()) } else { Message::assistant_with_tool_calls_and_reasoning(content, atc, reasoning_content.clone()) };
        if !is_empty { self.messages.push(msg); }
        self.last_reasoning_content = reasoning_content;
        self.tool_calls = fixed_tool_calls;
        self.usage = usage;
        self.total_usage = total_usage;
        self.message_count_after_last_think = Some(self.messages.len());
        self.think_count = self.think_count.saturating_add(1);
        self
    }
    pub fn last_assistant_reply(&self) -> Option<String> {
        self.messages.iter().rev().find_map(|m| match m { Message::Assistant(p) => Some(p.content.clone()), _ => None })
    }
    pub fn last_reasoning_content(&self) -> Option<String> { self.last_reasoning_content.clone() }
}

fn compute_think_usage(t: Option<&LlmUsage>, r: Option<&LlmUsage>) -> (Option<LlmUsage>, Option<LlmUsage>) {
    match (t, r) {
        (Some(t), Some(r)) => (Some(r.clone()), Some(t.accumulate(r))),
        (None, Some(r)) => (Some(r.clone()), Some(r.clone())),
        (Some(t), None) => (None, Some(t.clone())),
        (None, None) => (None, None),
    }
}

/// Metadata stored alongside ReAct checkpoints.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReActCheckpointMeta {
    pub summary: Option<String>,
}


/// Normalized tool output result (from tool_output_normalizer).
#[derive(Debug, Clone, Default)]
pub struct NormalizedToolOutput {
    pub raw_content: Option<String>,
    pub observation_text: String,
    pub display_text: String,
    pub storage_ref: Option<ToolStorageRef>,
    pub strategy: ToolOutputStrategy,
    pub raw_chars: usize,
    pub observation_chars: usize,
    pub truncated: bool,
    pub is_error: bool,
}

impl From<NormalizedToolOutput> for ToolResult {
    fn from(n: NormalizedToolOutput) -> Self {
        Self {
            raw_content: n.raw_content,
            observation_text: if n.observation_text.is_empty() { None } else { Some(n.observation_text) },
            display_text: if n.display_text.is_empty() { None } else { Some(n.display_text) },
            storage_ref: n.storage_ref,
            strategy: Some(n.strategy),
            raw_chars: Some(n.raw_chars),
            observation_chars: Some(n.observation_chars),
            truncated: n.truncated,
            is_error: n.is_error,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use loom_llm::LlmUsage;
    use loom_llm::ToolCall;
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

    #[test]
    fn apply_think_skips_empty_content_no_reasoning_no_tools() {
        let state = ReActState::default();
        let next = state.apply_think("".to_string(), None, vec![], None);
        assert!(next.messages.is_empty());
        assert_eq!(next.think_count, 1);
    }

    #[test]
    fn apply_think_skips_whitespace_only_content() {
        let state = ReActState::default();
        let next = state.apply_think("   ".to_string(), None, vec![], None);
        assert!(next.messages.is_empty());
    }

    #[test]
    fn apply_think_keeps_message_with_reasoning_fallback() {
        let state = ReActState::default();
        let next = state.apply_think("".to_string(), Some("thinking".to_string()), vec![], None);
        assert_eq!(next.messages.len(), 1);
    }

    #[test]
    fn apply_think_keeps_message_with_tool_calls() {
        let state = ReActState::default();
        let next = state.apply_think(
            "".to_string(),
            None,
            vec![ToolCall {
                name: "fn".into(),
                arguments: "{}".into(),
                id: Some("c1".into()),
            }],
            None,
        );
        assert_eq!(next.messages.len(), 1);
    }

    #[test]
    fn apply_think_skips_empty_reasoning_string() {
        let state = ReActState::default();
        let next = state.apply_think("".to_string(), Some("   ".to_string()), vec![], None);
        assert!(next.messages.is_empty());
    }
}
