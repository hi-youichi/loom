//! Build the compression subgraph (prune → compact → END) and a node wrapper to use it in a parent graph.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::{
    CompilationError, CompiledStateGraph, Next, Node, RunContext, StateGraph, END, START,
};
use crate::llm::LlmProvider;

use crate::state::ReActState;

use super::compact_node::CompactNode;
use super::config::CompactionConfig;
use super::prune_node::PruneNode;

/// Builds the compression subgraph: prune → compact → END.
pub fn build_graph(
    config: CompactionConfig,
    provider: Arc<dyn LlmProvider>,
    compact_llm: Option<Arc<dyn LlmProvider>>,
) -> Result<CompiledStateGraph<ReActState>, CompilationError> {
    let prune_node = Arc::new(PruneNode {
        config: config.clone(),
    });
    let compact_provider = compact_llm.unwrap_or(provider);
    let default_model = compact_provider.default_model().to_string();
    let compact_client = compact_provider.create_client(&default_model).expect("failed to create compact client");
    let compact_node = Arc::new(CompactNode {
        config,
        llm: Arc::from(compact_client),
    });
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

    use crate::message::{Message, UserContent};
    use crate::state::ReActState;
    use crate::MockLlm;

    use super::*;

    use crate::llm::{FixedLlmProvider, LlmProvider};

    fn mock_provider() -> Arc<dyn LlmProvider> {
        Arc::new(FixedLlmProvider {
            client: Arc::new(MockLlm::with_no_tool_calls("")),
            model_id: "mock".to_string(),
        })
    }

    #[test]
    fn build_graph_compiles() {
        let _compiled = build_graph(CompactionConfig::default(), mock_provider(), None).expect("compile");
    }

    #[tokio::test]
    async fn build_graph_invoke_preserves_messages_when_no_prune_no_overflow() {
        let compiled = build_graph(CompactionConfig::default(), mock_provider(), None).expect("compile");
        let state = ReActState {
            messages: vec![Message::user("hello")],
            ..Default::default()
        };
        let out = compiled.invoke(state, None).await.unwrap();
        assert_eq!(out.messages.len(), 1);
        assert!(matches!(&out.messages[0], Message::User(UserContent::Text(s)) if s == "hello"));
    }

    #[tokio::test]
    async fn compression_graph_node_id_is_compress() {
        let inner = build_graph(CompactionConfig::default(), mock_provider(), None).unwrap();
        let node = CompressionGraphNode::new(inner);
        assert_eq!(node.id(), "compress");
    }

    #[tokio::test]
    async fn compression_graph_node_run_invokes_inner() {
        let inner = build_graph(CompactionConfig::default(), mock_provider(), None).unwrap();
        let node = CompressionGraphNode::new(inner);
        let state = ReActState {
            messages: vec![Message::user("test")],
            turn_count: 1,
            think_count: 1,
            ..Default::default()
        };
        let (out, next) = node.run(state).await.unwrap();
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.turn_count, 1);
        assert!(matches!(next, Next::Continue));
    }
}
