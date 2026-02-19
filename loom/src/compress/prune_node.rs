//! Prune node: runs compaction::prune on state.messages when config.prune is true.

use async_trait::async_trait;

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
        let messages = if self.config.prune {
            compaction::prune(state.messages, &self.config)
        } else {
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
            config: CompactionConfig { prune: false, ..Default::default() },
        };
        let state = ReActState {
            messages: vec![Message::User("Tool x returned: y".to_string())],
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
        assert!(matches!(&out.messages[0], Message::User(s) if s.contains("Tool x returned:")));
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
                Message::User("u".to_string()),
                Message::User("Tool a returned: xxxxxxxxxxxxxxxxxxxx".to_string()), // 5 tokens
            ],
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: 0,
            approval_result: None,
            usage: None,
            total_usage: None,
            message_count_after_last_think: None,
        };
        let (out, next) = node.run(state).await.unwrap();
        assert_eq!(out.messages.len(), 2);
        assert!(matches!(&out.messages[1], Message::User(s) if s == compaction::PRUNE_PLACEHOLDER));
        assert!(matches!(next, Next::Continue));
    }
}
