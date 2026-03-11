//! Compact node: when config.auto and context overflows, summarizes old messages via LLM.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::debug;

use crate::error::AgentError;
use crate::graph::{Next, Node};
use crate::llm::LlmClient;
use crate::state::ReActState;

use super::compaction;
use super::config::CompactionConfig;
use super::context_window;

/// Node that compacts conversation history when context overflows (config.auto).
pub struct CompactNode {
    pub config: CompactionConfig,
    pub llm: Arc<dyn LlmClient>,
}

#[async_trait]
impl Node<ReActState> for CompactNode {
    fn id(&self) -> &str {
        "compact"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        let message_count = state.messages.len();
        debug!(
            message_count,
            auto = self.config.auto,
            max_context_tokens = self.config.max_context_tokens,
            reserve_tokens = self.config.reserve_tokens,
            "compress compact node entered"
        );

        let overflow_input = context_window::ContextWindowCheck {
            messages: &state.messages,
            usage: state
                .usage
                .as_ref()
                .map(|u| (u.prompt_tokens, u.completion_tokens)),
            message_count_after_last_think: state.message_count_after_last_think,
            max_context_tokens: self.config.max_context_tokens,
            reserve_tokens: self.config.reserve_tokens,
        };
        let current_tokens = context_window::current_tokens(&overflow_input);
        let overflow = context_window::is_overflow(&overflow_input);
        debug!(
            current_tokens,
            overflow,
            max_context_tokens = self.config.max_context_tokens,
            reserve_tokens = self.config.reserve_tokens,
            "context window check"
        );

        let messages = if self.config.auto && overflow {
            compaction::compact(&state.messages, self.llm.as_ref(), &self.config).await?
        } else {
            let reason = if !self.config.auto {
                "auto_disabled"
            } else {
                "no_overflow"
            };
            debug!(reason, current_tokens, "compact skipped");
            state.messages
        };
        Ok((ReActState { messages, ..state }, Next::Continue))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::message::Message;
    use crate::state::ReActState;
    use crate::MockLlm;

    use super::*;

    #[tokio::test]
    async fn compact_node_id_is_compact() {
        let node = CompactNode {
            config: CompactionConfig::default(),
            llm: Arc::new(MockLlm::with_no_tool_calls("")),
        };
        assert_eq!(node.id(), "compact");
    }

    #[tokio::test]
    async fn compact_node_auto_false_passes_through() {
        let node = CompactNode {
            config: CompactionConfig {
                auto: false,
                ..Default::default()
            },
            llm: Arc::new(MockLlm::with_no_tool_calls("")),
        };
        let state = ReActState {
            messages: vec![Message::User("a".repeat(200_000))], // would overflow if checked
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: 0,
            approval_result: None,
            usage: None,
            total_usage: None,
            message_count_after_last_think: None,
        };
        let (out, next) = node.run(state).await.unwrap();
        assert_eq!(out.messages.len(), 1);
        assert!(matches!(next, Next::Continue));
    }

    #[tokio::test]
    async fn compact_node_auto_true_but_no_overflow_passes_through() {
        let node = CompactNode {
            config: CompactionConfig {
                auto: true,
                max_context_tokens: 200_000,
                reserve_tokens: 4096,
                ..Default::default()
            },
            llm: Arc::new(MockLlm::with_no_tool_calls("")),
        };
        let state = ReActState {
            messages: vec![Message::User("short".to_string())],
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: 0,
            approval_result: None,
            usage: None,
            total_usage: None,
            message_count_after_last_think: None,
        };
        let (out, next) = node.run(state).await.unwrap();
        assert_eq!(out.messages.len(), 1);
        assert!(matches!(next, Next::Continue));
    }
}
