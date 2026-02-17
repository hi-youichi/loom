//! DUP graph runner: build, initial state, invoke and stream.
//!
//! Graph: START → understand → plan → [tools_condition] → act | end, observe → plan.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::AgentError;
use crate::graph::{CompilationError, CompiledStateGraph, LoggingNodeMiddleware};
use crate::helve::ApprovalPolicy;
use crate::memory::{CheckpointError, Checkpointer, RunnableConfig, Store};
use crate::message::Message;
use crate::react::{build_react_initial_state, REACT_SYSTEM_PROMPT};
use crate::runner_common;
use crate::stream::StreamEvent;
use crate::tool_source::ToolSource;
use crate::LlmClient;
use crate::{StateGraph, END, START};

use super::adapter_nodes::{DupActNode, DupObserveNode, PlanNode};
use super::state::DupState;
use super::understand_node::UnderstandNode;

/// Condition for DUP graph: route based on state.core.tool_calls.
fn dup_tools_condition(state: &DupState) -> &'static str {
    if state.core.tool_calls.is_empty() {
        END
    } else {
        "act"
    }
}

/// Builds the initial DupState for a run.
///
/// Uses build_react_initial_state for core, sets understood to None.
pub async fn build_dup_initial_state(
    user_message: &str,
    checkpointer: Option<&dyn Checkpointer<DupState>>,
    runnable_config: Option<&RunnableConfig>,
    system_prompt: Option<&str>,
) -> Result<DupState, CheckpointError> {
    let load_from_checkpoint =
        checkpointer.is_some() && runnable_config.and_then(|c| c.thread_id.as_ref()).is_some();

    if load_from_checkpoint {
        let cp = checkpointer.expect("checkpointer is Some");
        let config = runnable_config.expect("runnable_config is Some");
        let tuple = cp.get_tuple(config).await?;
        if let Some((checkpoint, _)) = tuple {
            let mut state = checkpoint.channel_values.clone();
            state
                .core
                .messages
                .push(Message::user(user_message.to_string()));
            state.core.tool_calls = vec![];
            state.core.tool_results = vec![];
            return Ok(state);
        }
    }

    let core = build_react_initial_state(
        user_message,
        None, // DUP uses DupState checkpointer, not ReActState
        runnable_config,
        Some(system_prompt.unwrap_or(REACT_SYSTEM_PROMPT)),
    )
    .await?;

    Ok(DupState {
        core,
        understood: None,
    })
}

/// Error type for DupRunner operations.
#[derive(Debug, thiserror::Error)]
pub enum DupRunError {
    #[error("compilation failed: {0}")]
    Compilation(#[from] CompilationError),
    #[error("checkpoint error: {0}")]
    Checkpoint(#[from] CheckpointError),
    #[error("execution failed: {0}")]
    Execution(#[from] AgentError),
    #[error("stream ended without final state")]
    StreamEndedWithoutState,
}

/// DUP graph runner: encapsulates compiled graph and persistence.
pub struct DupRunner {
    compiled: CompiledStateGraph<DupState>,
    checkpointer: Option<Arc<dyn Checkpointer<DupState>>>,
    runnable_config: Option<RunnableConfig>,
    system_prompt: Option<String>,
}

/// Wraps Arc<dyn LlmClient> to share one LLM between UnderstandNode and PlanNode.
struct SharedLlm(Arc<dyn LlmClient>);

#[async_trait::async_trait]
impl LlmClient for SharedLlm {
    async fn invoke(
        &self,
        messages: &[crate::message::Message],
    ) -> Result<crate::llm::LlmResponse, crate::error::AgentError> {
        self.0.invoke(messages).await
    }
    async fn invoke_stream(
        &self,
        messages: &[crate::message::Message],
        tx: Option<tokio::sync::mpsc::Sender<crate::stream::MessageChunk>>,
    ) -> Result<crate::llm::LlmResponse, crate::error::AgentError> {
        self.0.invoke_stream(messages, tx).await
    }
}

impl DupRunner {
    /// Creates a DUP runner with the given LLM, tool source, and optional persistence.
    pub fn new(
        llm: Arc<dyn LlmClient>,
        tool_source: Box<dyn ToolSource>,
        checkpointer: Option<Arc<dyn Checkpointer<DupState>>>,
        store: Option<Arc<dyn Store>>,
        runnable_config: Option<RunnableConfig>,
        system_prompt: Option<String>,
        approval_policy: Option<ApprovalPolicy>,
        verbose: bool,
    ) -> Result<Self, CompilationError> {
        let understand = UnderstandNode::new(Box::new(SharedLlm(Arc::clone(&llm))));
        let plan = PlanNode::new(Box::new(SharedLlm(llm)));
        let act = DupActNode::new(tool_source).with_approval_policy(approval_policy);
        let observe = DupObserveNode::new();

        let mut graph = StateGraph::<DupState>::new();
        if let Some(s) = store {
            graph = graph.with_store(s);
        }

        let plan_condition_path_map: HashMap<String, String> =
            [("act".into(), "act".into()), (END.into(), END.into())]
                .into_iter()
                .collect();

        graph
            .add_node("understand", Arc::new(understand))
            .add_node("plan", Arc::new(plan))
            .add_node("act", Arc::new(act))
            .add_node("observe", Arc::new(observe))
            .add_edge(START, "understand")
            .add_edge("understand", "plan")
            .add_conditional_edges(
                "plan",
                Arc::new(|state: &DupState| dup_tools_condition(state).to_string()),
                Some(plan_condition_path_map),
            )
            .add_edge("act", "observe")
            .add_edge("observe", "plan");

        let graph = if verbose {
            graph.with_middleware(Arc::new(LoggingNodeMiddleware::<DupState>::default()))
        } else {
            graph
        };

        let compiled = match (&checkpointer, verbose) {
            (Some(cp), true) => {
                let mw = Arc::new(LoggingNodeMiddleware::<DupState>::default());
                graph.compile_with_checkpointer_and_middleware(Arc::clone(cp), mw)?
            }
            (Some(cp), false) => graph.compile_with_checkpointer(Arc::clone(cp))?,
            (None, _) => graph.compile()?,
        };

        Ok(Self {
            compiled,
            checkpointer,
            runnable_config,
            system_prompt,
        })
    }

    /// Invokes the graph with the given user message.
    pub async fn invoke(&self, user_message: &str) -> Result<DupState, DupRunError> {
        self.invoke_with_config(user_message, None).await
    }

    /// Invokes with optional per-invoke config.
    pub async fn invoke_with_config(
        &self,
        user_message: &str,
        config: Option<RunnableConfig>,
    ) -> Result<DupState, DupRunError> {
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_dup_initial_state(
            user_message,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
            self.system_prompt.as_deref(),
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
    ) -> Result<DupState, DupRunError>
    where
        F: FnMut(StreamEvent<DupState>),
    {
        self.stream_with_config(user_message, None, on_event).await
    }

    /// Streams with optional per-invoke config.
    pub async fn stream_with_config<F>(
        &self,
        user_message: &str,
        config: Option<RunnableConfig>,
        on_event: Option<F>,
    ) -> Result<DupState, DupRunError>
    where
        F: FnMut(StreamEvent<DupState>),
    {
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_dup_initial_state(
            user_message,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
            self.system_prompt.as_deref(),
        )
        .await?;
        runner_common::run_stream_with_config(&self.compiled, state, run_config, on_event)
            .await
            .map_err(|_| DupRunError::StreamEndedWithoutState)
    }
}
