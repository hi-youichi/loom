//! Build the compression subgraph (prune → compact → END) and a node wrapper to use it in a parent graph.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::{
    CompilationError, CompiledStateGraph, Next, Node, RunContext, StateGraph, END, START,
};
use crate::llm::LlmClient;
use crate::state::ReActState;

use super::compact_node::CompactNode;
use super::config::CompactionConfig;
use super::prune_node::PruneNode;

/// Builds the compression subgraph: prune → compact → END.
pub fn build_graph(
    config: CompactionConfig,
    llm: Arc<dyn LlmClient>,
) -> Result<CompiledStateGraph<ReActState>, CompilationError> {
    let prune_node = Arc::new(PruneNode {
        config: config.clone(),
    });
    let compact_node = Arc::new(CompactNode { config, llm });
    let mut graph = StateGraph::<ReActState>::new();
    graph
        .add_node("prune", prune_node)
        .add_node("compact", compact_node)
        .add_edge(START, "prune")
        .add_edge("prune", "compact")
        .add_edge("compact", END);
    graph.compile()
}

/// Wraps a compiled compression graph so it can be used as a node (observe → compress → think).
pub struct CompressionGraphNode {
    inner: CompiledStateGraph<ReActState>,
}

impl CompressionGraphNode {
    pub fn new(inner: CompiledStateGraph<ReActState>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Node<ReActState> for CompressionGraphNode {
    fn id(&self) -> &str {
        "compress"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        let new_state = self.inner.invoke(state, None).await?;
        Ok((new_state, Next::Continue))
    }

    async fn run_with_context(
        &self,
        state: ReActState,
        ctx: &RunContext<ReActState>,
    ) -> Result<(ReActState, Next), AgentError> {
        let config = Some(ctx.config.clone());
        let new_state = self.inner.invoke(state, config).await?;
        Ok((new_state, Next::Continue))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::message::Message;
    use crate::state::ReActState;
    use crate::MockLlm;

    use super::*;

    #[test]
    fn build_graph_compiles() {
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlm::with_no_tool_calls(""));
        let _compiled = build_graph(CompactionConfig::default(), llm).expect("compile");
    }

    #[tokio::test]
    async fn build_graph_invoke_preserves_messages_when_no_prune_no_overflow() {
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlm::with_no_tool_calls(""));
        let compiled = build_graph(CompactionConfig::default(), llm).expect("compile");
        let state = ReActState {
            messages: vec![Message::User("hello".to_string())],
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: 0,
            approval_result: None,
            usage: None,
            total_usage: None,
            message_count_after_last_think: None,
        };
        let out = compiled.invoke(state, None).await.unwrap();
        assert_eq!(out.messages.len(), 1);
        assert!(matches!(&out.messages[0], Message::User(s) if s == "hello"));
    }

    #[tokio::test]
    async fn compression_graph_node_id_is_compress() {
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlm::with_no_tool_calls(""));
        let inner = build_graph(CompactionConfig::default(), llm).unwrap();
        let node = CompressionGraphNode::new(inner);
        assert_eq!(node.id(), "compress");
    }

    #[tokio::test]
    async fn compression_graph_node_run_invokes_inner() {
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlm::with_no_tool_calls(""));
        let inner = build_graph(CompactionConfig::default(), llm).unwrap();
        let node = CompressionGraphNode::new(inner);
        let state = ReActState {
            messages: vec![Message::User("test".to_string())],
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: 1,
            approval_result: None,
            usage: None,
            total_usage: None,
            message_count_after_last_think: None,
        };
        let (out, next) = node.run(state).await.unwrap();
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.turn_count, 1);
        assert!(matches!(next, Next::Continue));
    }
}
