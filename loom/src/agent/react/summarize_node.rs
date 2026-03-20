//! Summarize node: generate session summary after first think.
//!
//! This node runs once after the first think to create a human-readable
//! summary of the conversation for session list display.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::Next;
use crate::llm::LlmClient;
use crate::message::Message;
use crate::state::ReActState;
use crate::Node;

/// Node that generates a session summary after the first think.
///
/// Uses a separate LLM call to create a concise summary (≤50 chars)
/// suitable for display in session lists.
pub struct SummarizeNode {
    llm: Arc<dyn LlmClient>,
}

impl SummarizeNode {
    /// Creates a new SummarizeNode with the given LLM client.
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl Node<ReActState> for SummarizeNode {
    fn id(&self) -> &str {
        "summarize"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        // Only generate summary if not already present
        if state.summary.is_some() {
            return Ok((state, Next::Continue));
        }

        // Extract user messages for summary generation
        let user_messages: Vec<_> = state
            .messages
            .iter()
            .filter_map(|m| match m {
                Message::User(content) => Some(content.clone()),
                _ => None,
            })
            .take(3)
            .collect();

        if user_messages.is_empty() {
            return Ok((state, Next::Continue));
        }

        // Generate summary using LLM
        let prompt = format!(
            r#"用一句话总结这个对话的主题（不超过50字，用对话的语言）：

{}

只输出摘要内容，不要其他内容。"#,
            user_messages.join("\n")
        );

        // Create a minimal message list for the summary request
        let summary_messages = vec![
            Message::system("You are a helpful assistant that creates concise conversation summaries."),
            Message::user(prompt),
        ];

        match self.llm.invoke(&summary_messages).await {
            Ok(response) => {
                let summary = response.content.trim().to_string();
                let summary = if summary.len() > 60 {
                    format!("{}...", &summary[..57])
                } else {
                    summary
                };

                // Update state with summary
                let new_state = ReActState {
                    messages: state.messages,
                    last_reasoning_content: state.last_reasoning_content,
                    tool_calls: state.tool_calls,
                    tool_results: state.tool_results,
                    turn_count: state.turn_count,
                    approval_result: state.approval_result,
                    usage: state.usage,
                    total_usage: state.total_usage,
                    message_count_after_last_think: state.message_count_after_last_think,
                    summary: Some(summary),
                    think_count: state.think_count,
                };

                Ok((new_state, Next::Continue))
            }
            Err(e) => {
                // Log error but don't fail the entire flow
                tracing::warn!("Failed to generate session summary: {}", e);
                Ok((state, Next::Continue))
            }
        }
    }
}

/// Determines if this is the first think (should route to summarize).
///
/// Returns true when:
/// - think_count == 1 (first think just completed)
/// - summary is not yet set
pub fn is_first_think(state: &ReActState) -> bool {
    state.think_count == 1 && state.summary.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_first_think_true() {
        let state = ReActState {
            messages: vec![Message::user("Hello")],
            think_count: 1,
            summary: None,
            ..Default::default()
        };
        assert!(is_first_think(&state));
    }

    #[test]
    fn test_is_first_think_false_already_summarized() {
        let state = ReActState {
            messages: vec![Message::user("Hello")],
            think_count: 1,
            summary: Some("Summary".to_string()),
            ..Default::default()
        };
        assert!(!is_first_think(&state));
    }

    #[test]
    fn test_is_first_think_false_second_think() {
        let state = ReActState {
            messages: vec![Message::user("Hello")],
            think_count: 2,
            summary: None,
            ..Default::default()
        };
        assert!(!is_first_think(&state));
    }
}
