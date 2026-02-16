//! GoT graph runner: build, initial state, invoke and stream.
//!
//! Graph: START → plan_graph → execute_graph → [has_pending] → execute_graph | END.

use std::collections::HashSet;
use std::sync::Arc;

use tokio_stream::StreamExt;

use crate::error::AgentError;
use crate::graph::{CompilationError, CompiledStateGraph, LoggingNodeMiddleware};
use crate::memory::{CheckpointError, Checkpointer, RunnableConfig, Store};
use crate::stream::{StreamEvent, StreamMode};
use crate::tool_source::ToolSource;
use crate::LlmClient;
use crate::{StateGraph, END, START};

use super::dag::ready_nodes;
use super::execute_engine::ExecuteGraphNode;
use super::plan_node::PlanGraphNode;
use super::state::GotState;

/// Condition: if there are ready or pending nodes, continue to execute_graph; else END.
fn got_execute_condition(state: &GotState) -> &'static str {
    let ready = ready_nodes(&state.task_graph, &state.node_states);
    if ready.is_empty() {
        END
    } else {
        "execute_graph"
    }
}

/// Builds the initial GotState for a run.
pub async fn build_got_initial_state(
    user_message: &str,
    checkpointer: Option<&dyn Checkpointer<GotState>>,
    runnable_config: Option<&RunnableConfig>,
) -> Result<GotState, CheckpointError> {
    if let (Some(cp), Some(config)) = (checkpointer, runnable_config) {
        if config.thread_id.is_some() {
            let tuple = cp.get_tuple(config).await?;
            if let Some((checkpoint, _)) = tuple {
                let mut state = checkpoint.channel_values.clone();
                state.input_message = user_message.to_string();
                return Ok(state);
            }
        }
    }

    Ok(GotState {
        input_message: user_message.to_string(),
        task_graph: super::state::TaskGraph::default(),
        node_states: std::collections::HashMap::new(),
    })
}

/// Error type for GotRunner operations.
#[derive(Debug, thiserror::Error)]
pub enum GotRunError {
    #[error("compilation failed: {0}")]
    Compilation(#[from] CompilationError),
    #[error("checkpoint error: {0}")]
    Checkpoint(#[from] CheckpointError),
    #[error("execution failed: {0}")]
    Execution(#[from] AgentError),
    #[error("stream ended without final state")]
    StreamEndedWithoutState,
}

/// GoT graph runner: encapsulates compiled graph and optional persistence.
pub struct GotRunner {
    compiled: CompiledStateGraph<GotState>,
    checkpointer: Option<Arc<dyn Checkpointer<GotState>>>,
    runnable_config: Option<RunnableConfig>,
}

impl GotRunner {
    /// Creates a GoT runner with the given LLM, tool source, and optional persistence.
    ///
    /// When `adaptive` is true, enables AGoT: complex nodes may be expanded into subgraphs
    /// after completion.
    /// When `agot_llm_complexity` is true, use LLM to decide simple vs complex instead of heuristic.
    pub fn new(
        llm: Arc<dyn LlmClient>,
        tool_source: Box<dyn ToolSource>,
        checkpointer: Option<Arc<dyn Checkpointer<GotState>>>,
        store: Option<Arc<dyn Store>>,
        runnable_config: Option<RunnableConfig>,
        verbose: bool,
        adaptive: bool,
        agot_llm_complexity: bool,
    ) -> Result<Self, CompilationError> {
        let plan = PlanGraphNode::new(Box::new(super::runner::SharedLlm(Arc::clone(&llm))));
        let execute = ExecuteGraphNode::new(llm, tool_source, adaptive, agot_llm_complexity);

        let mut graph = StateGraph::<GotState>::new();
        if let Some(s) = store {
            graph = graph.with_store(s);
        }

        let condition_path_map: std::collections::HashMap<String, String> = [
            ("execute_graph".into(), "execute_graph".into()),
            (END.into(), END.into()),
        ]
        .into_iter()
        .collect();

        graph
            .add_node("plan_graph", Arc::new(plan))
            .add_node("execute_graph", Arc::new(execute))
            .add_edge(START, "plan_graph")
            .add_edge("plan_graph", "execute_graph")
            .add_conditional_edges(
                "execute_graph",
                Arc::new(|state: &GotState| got_execute_condition(state).to_string()),
                Some(condition_path_map),
            );

        let graph = if verbose {
            graph.with_middleware(Arc::new(LoggingNodeMiddleware::<GotState>::default()))
        } else {
            graph
        };

        let compiled = match (&checkpointer, verbose) {
            (Some(cp), true) => {
                let mw = Arc::new(LoggingNodeMiddleware::<GotState>::default());
                graph.compile_with_checkpointer_and_middleware(Arc::clone(cp), mw)?
            }
            (Some(cp), false) => graph.compile_with_checkpointer(Arc::clone(cp))?,
            (None, _) => graph.compile()?,
        };

        Ok(Self {
            compiled,
            checkpointer,
            runnable_config,
        })
    }

    /// Invokes the graph with the given user message.
    pub async fn invoke(&self, user_message: &str) -> Result<GotState, GotRunError> {
        self.invoke_with_config(user_message, None).await
    }

    /// Invokes with optional per-invoke config.
    pub async fn invoke_with_config(
        &self,
        user_message: &str,
        config: Option<RunnableConfig>,
    ) -> Result<GotState, GotRunError> {
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_got_initial_state(
            user_message,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
        )
        .await?;
        let final_state = self.compiled.invoke(state, run_config).await?;
        Ok(final_state)
    }

    /// Streams the graph execution; returns the final state.
    pub async fn stream_with_callback<F>(
        &self,
        user_message: &str,
        on_event: Option<F>,
    ) -> Result<GotState, GotRunError>
    where
        F: FnMut(StreamEvent<GotState>),
    {
        self.stream_with_config(user_message, None, on_event).await
    }

    /// Streams with optional per-invoke config.
    pub async fn stream_with_config<F>(
        &self,
        user_message: &str,
        config: Option<RunnableConfig>,
        mut on_event: Option<F>,
    ) -> Result<GotState, GotRunError>
    where
        F: FnMut(StreamEvent<GotState>),
    {
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_got_initial_state(
            user_message,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
        )
        .await?;

        let modes = HashSet::from([
            StreamMode::Messages,
            StreamMode::Tasks,
            StreamMode::Updates,
            StreamMode::Values,
            StreamMode::Custom,
        ]);
        let mut stream = self.compiled.stream(state, run_config, modes);

        let mut final_state: Option<GotState> = None;
        while let Some(event) = stream.next().await {
            if let Some(ref mut f) = on_event {
                f(event.clone());
            }
            if let StreamEvent::Values(s) = event {
                final_state = Some(s);
            }
        }

        final_state.ok_or(GotRunError::StreamEndedWithoutState)
    }
}

/// Wraps Arc<dyn LlmClient> to share one LLM between PlanGraphNode and ExecuteGraphNode.
pub(super) struct SharedLlm(Arc<dyn LlmClient>);

#[async_trait::async_trait]
impl LlmClient for SharedLlm {
    async fn invoke(
        &self,
        messages: &[crate::message::Message],
    ) -> Result<crate::llm::LlmResponse, AgentError> {
        self.0.invoke(messages).await
    }
    async fn invoke_stream(
        &self,
        messages: &[crate::message::Message],
        tx: Option<tokio::sync::mpsc::Sender<crate::stream::MessageChunk>>,
    ) -> Result<crate::llm::LlmResponse, AgentError> {
        self.0.invoke_stream(messages, tx).await
    }
}
