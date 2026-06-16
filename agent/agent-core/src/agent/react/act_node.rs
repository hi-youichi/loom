//! Act node: one ReAct step that executes tool_calls and produces tool_results.
//!
//! `ActNode` is a thin `Node<ReActState>` adapter — all execution logic lives
//! in [`ToolCallExecutor`].

use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, warn};

use loom_cli_types::RunCancellation;
use loom_graph::{Next, Node, RunnableConfig, RunContext};
use loom_llm::error::AgentError;
use loom_stream::StreamMode;
use loom_types::state::ReActState;
use tool_core::ToolRegistryLocked;

use super::act_executor::ToolCallExecutor;

/// Act node: one ReAct step that executes tool_calls and produces tool_results.
#[allow(clippy::type_complexity)]
pub struct ActNode {
    tools: Arc<ToolRegistryLocked>,
    run_cancellation: Option<RunCancellation>,
    any_stream_event_sender: Option<Arc<dyn Fn(loom_cli_types::AnyStreamEvent) + Send + Sync>>,
}

impl ActNode {
    pub fn new(tools: Arc<ToolRegistryLocked>) -> Self {
        Self {
            tools,
            run_cancellation: None,
            any_stream_event_sender: None,
        }
    }

    pub fn with_run_cancellation(mut self, rc: Option<RunCancellation>) -> Self {
        self.run_cancellation = rc;
        self
    }

    pub fn with_any_stream_event_sender(
        mut self,
        sender: Option<Arc<dyn Fn(loom_cli_types::AnyStreamEvent) + Send + Sync>>,
    ) -> Self {
        self.any_stream_event_sender = sender;
        self
    }
}

#[async_trait]
impl Node<ReActState> for ActNode {
    fn id(&self) -> &str {
        "act"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        self.run_with_context(state, &RunContext::new(RunnableConfig::default()))
            .await
    }

    async fn run_with_context(
        &self,
        state: ReActState,
        run_ctx: &RunContext<ReActState>,
    ) -> Result<(ReActState, Next), AgentError> {
        let tools_mode = run_ctx.stream_mode.contains(&StreamMode::Tools)
            || run_ctx.stream_mode.contains(&StreamMode::Debug);

        debug!(
            tool_calls = state.tool_calls.len(),
            tools_mode,
            "act:input"
        );
        for (i, tc) in state.tool_calls.iter().enumerate() {
            debug!(
                "  tool_call[{}] id={:?} name={} args_len={}",
                i, tc.id, tc.name, tc.arguments.len()
            );
        }

        let executor = ToolCallExecutor::new(
            self.tools.clone(),
            state.messages.clone(),
            run_ctx,
            self.run_cancellation.clone(),
            self.any_stream_event_sender.clone(),
        )
        .await;
        let tool_results = executor.execute(&state.tool_calls).await?;

        debug!(
            tool_calls = state.tool_calls.len(),
            tool_results = tool_results.len(),
            "act:summary"
        );
        for tr in &tool_results {
            let label = if tr.is_error { "error" } else { "ok" };
            debug!(
                "  {} id={:?} name={:?} → {} len={}",
                label,
                tr.call_id,
                tr.name,
                label,
                tr.observation().len()
            );
        }
        if tool_results.len() != state.tool_calls.len() {
            warn!(
                tool_calls = state.tool_calls.len(),
                tool_results = tool_results.len(),
                "act:mismatch tool_calls vs tool_results count"
            );
        }

        Ok((ReActState { tool_results, ..state }, Next::Continue))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn act_node_id() {
        let node = ActNode::new(tool_core::mock_registry());
        assert_eq!(Node::<ReActState>::id(&node), "act");
    }
}
