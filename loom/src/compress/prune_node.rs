//! Prune node: runs compaction::prune on state.messages when config.prune is true.

use async_trait::async_trait;
use tracing::debug;

use crate::error::AgentError;
use crate::graph::{Next, Node};

use crate::state::ReActState;

use super::compaction;
use super::config::CompactionConfig;

/// Node that prunes old tool results from messages when `config.prune` is true.
pub struct PruneNode {
    pub config: CompactionConfig,
}

#[async_trait]
impl Node<ReActState> for PruneNode {
    fn id(&self) -> &str {
        "prune"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        let message_count = state.messages.len();
        debug!(
            message_count,
            prune = self.config.prune,
            "compress prune node entered"
        );
        let messages = if self.config.prune {
            compaction::prune(state.messages, &self.config)
        } else {
            debug!("prune disabled, passing through");
            state.messages
        };
        Ok((ReActState { messages, ..state }, Next::Continue))
    }
}

#[cfg(test)]
mod tests {
    use crate::message::Message;
    use crate::state::ReActState;

    use super::*;

    #[tokio::test]
    async fn prune_node_id_is_prune() {
        let node = PruneNode {
            config: CompactionConfig::default(),
        };
        assert_eq!(node.id(), "prune");
    }

    #[tokio::test]
    async fn prune_node_with_prune_false_passes_through() {
        let node = PruneNode {
            config: CompactionConfig {
                prune: false,
                ..Default::default()
            },
        };
        let state = ReActState {
            messages: vec![Message::user("Tool x returned: y")],
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
        };
        let (out, next) = node.run(state).await.unwrap();
        assert_eq!(out.messages.len(), 1);
        assert!(matches!(&out.messages[0], Message::User(UserContent::Text(s)) if s.contains("Tool x returned:")));
        assert!(matches!(next, Next::Continue));
    }

    #[tokio::test]
    async fn prune_node_with_prune_true_applies_prune() {
        let node = PruneNode {
            config: CompactionConfig {
                prune: true,
                prune_keep_tokens: 1,
                prune_minimum: Some(0),
                ..Default::default()
            },
        };
        let state = ReActState {
            messages: vec![
                Message::user("u"),
                Message::user("Tool a returned: xxxxxxxxxxxxxxxxxxxx"), // 5 tokens
            ],
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
        };
        let (out, next) = node.run(state).await.unwrap();
        assert_eq!(out.messages.len(), 2);
        assert!(matches!(&out.messages[1], Message::User(UserContent::Text(s)) if s == compaction::PRUNE_PLACEHOLDER));
        assert!(matches!(next, Next::Continue));
    }
}
