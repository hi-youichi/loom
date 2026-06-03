//! ReAct state and tool types for the minimal ReAct agent.
//!
//! Core types are now defined in the loom-types crate.
//! This module re-exports them for backward compatibility.

// Re-export core types from loom-types
pub use loom_types::state::{
    ModelConfig, ReActState, ToolResult, ToolStorageRef, ReActCheckpointMeta,
    NormalizedToolOutput, ToolOutputStrategy,
};

/// Alias for LLM tool call (from loom-llm crate).
pub type ToolCall = loom_llm::ToolCall;
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
