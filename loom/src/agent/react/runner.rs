//! ReAct graph runner: encapsulates graph build, initial state, invoke and stream.

use std::collections::HashMap;
use std::sync::Arc;

use crate::compress::{build_graph, CompactionConfig, CompressionGraphNode};
use crate::error::AgentError;
use crate::graph::{
    CompilationError, CompiledStateGraph, LoggingNodeMiddleware, StateGraph, END, START,
};
use crate::helve::ApprovalPolicy;
use crate::memory::{CheckpointError, Checkpointer, RunnableConfig, Store};
use crate::message::Message;
use crate::runner_common::{self, load_from_checkpoint_or_build};
use crate::state::ReActState;
use crate::stream::StreamEvent;
use crate::tool_source::ToolSource;
use crate::LlmClient;

use super::act_node::{ActNode, HandleToolErrors};
use super::observe_node::ObserveNode;
use super::think_node::ThinkNode;
use super::tools_condition;
use super::with_node_logging::WithNodeLogging;
use super::REACT_SYSTEM_PROMPT;

/// Optional configuration for [`run_agent`] and [`run_react_graph_stream`].
///
/// When a field is `None`, defaults are used: `llm` and `tool_source` default to
/// mock implementations (e.g. [`MockLlm::first_tools_then_end`], [`MockToolSource::get_time_example`])
/// so that `run_agent("What time is it?", None)` works for quick demos.
#[derive(Default)]
pub struct AgentOptions {
    /// LLM client. Defaults to a mock that returns one tool call then a final reply.
    pub llm: Option<Box<dyn LlmClient>>,
    /// Tool source. Defaults to a mock that provides `get_time`.
    pub tool_source: Option<Box<dyn ToolSource>>,
    /// Optional checkpointer for persisting/restoring conversation state.
    pub checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
    /// Optional long-term memory store (e.g. LanceDB).
    pub store: Option<Arc<dyn Store>>,
    /// Optional runtime config (thread_id, user_id, etc.).
    pub runnable_config: Option<RunnableConfig>,
    /// If true, log node and state details to stderr.
    pub verbose: bool,
}

struct ResolvedRunAgentOptions {
    llm: Box<dyn LlmClient>,
    tool_source: Box<dyn ToolSource>,
    checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
    store: Option<Arc<dyn Store>>,
    runnable_config: Option<RunnableConfig>,
    verbose: bool,
}

fn resolve_run_agent_options(opts: AgentOptions) -> ResolvedRunAgentOptions {
    let llm = opts
        .llm
        .unwrap_or_else(|| Box::new(crate::MockLlm::first_tools_then_end()));
    let tool_source = opts
        .tool_source
        .unwrap_or_else(|| Box::new(crate::MockToolSource::get_time_example()));
    ResolvedRunAgentOptions {
        llm,
        tool_source,
        checkpointer: opts.checkpointer,
        store: opts.store,
        runnable_config: opts.runnable_config,
        verbose: opts.verbose,
    }
}

pub async fn build_react_initial_state(
    user_message: &str,
    checkpointer: Option<&dyn Checkpointer<ReActState>>,
    runnable_config: Option<&RunnableConfig>,
    system_prompt: Option<&str>,
) -> Result<ReActState, CheckpointError> {
    let prompt = system_prompt.unwrap_or(REACT_SYSTEM_PROMPT);
    let user_message_owned = user_message.to_string();
    load_from_checkpoint_or_build(
        checkpointer,
        runnable_config,
        user_message,
        async move {
            Ok(ReActState {
                messages: vec![Message::system(prompt), Message::user(user_message_owned)],
                tool_calls: vec![],
                tool_results: vec![],
                turn_count: 0,
                approval_result: None,
                usage: None,
                total_usage: None,
                message_count_after_last_think: None,
            })
        },
        |mut state, msg| {
            state.messages.push(Message::user(msg));
            state.tool_calls = vec![];
            state.tool_results = vec![];
            state
        },
    )
    .await
}

pub async fn run_agent(
    user_message: &str,
    options: Option<AgentOptions>,
) -> Result<ReActState, RunError> {
    let opts = resolve_run_agent_options(options.unwrap_or_default());
    let runner = ReactRunner::new(
        opts.llm,
        opts.tool_source,
        opts.checkpointer,
        opts.store,
        opts.runnable_config,
        None,
        None,
        None,
        opts.verbose,
    )?;
    runner.invoke(user_message).await
}

pub async fn run_react_graph_stream<F>(
    user_message: &str,
    options: Option<AgentOptions>,
    on_event: Option<F>,
) -> Result<ReActState, RunError>
where
    F: FnMut(StreamEvent<ReActState>),
{
    let opts = resolve_run_agent_options(options.unwrap_or_default());
    let runner = ReactRunner::new(
        opts.llm,
        opts.tool_source,
        opts.checkpointer,
        opts.store,
        opts.runnable_config,
        None,
        None,
        None,
        opts.verbose,
    )?;
    runner.stream_with_callback(user_message, on_event).await
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("compilation failed: {0}")]
    Compilation(#[from] CompilationError),
    #[error("checkpoint error: {0}")]
    Checkpoint(#[from] CheckpointError),
    #[error("execution failed: {0}")]
    Execution(#[from] AgentError),
    #[error("stream ended without final state")]
    StreamEndedWithoutState,
}

impl From<std::io::Error> for RunError {
    fn from(e: std::io::Error) -> Self {
        RunError::Execution(AgentError::ExecutionFailed(e.to_string()))
    }
}

pub struct ReactRunner {
    compiled: CompiledStateGraph<ReActState>,
    checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
    runnable_config: Option<RunnableConfig>,
    system_prompt: Option<String>,
}

impl ReactRunner {
    pub fn new(
        llm: Box<dyn LlmClient>,
        tool_source: Box<dyn ToolSource>,
        checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
        store: Option<Arc<dyn Store>>,
        runnable_config: Option<RunnableConfig>,
        system_prompt: Option<String>,
        approval_policy: Option<ApprovalPolicy>,
        compaction_config: Option<CompactionConfig>,
        verbose: bool,
    ) -> Result<Self, CompilationError> {
        let llm = Arc::from(llm);
        let think = ThinkNode::new(Arc::clone(&llm));
        let act = ActNode::new(tool_source)
            .with_handle_tool_errors(HandleToolErrors::Always(None))
            .with_approval_policy(approval_policy);
        let observe = ObserveNode::with_loop();

        let compaction_cfg = compaction_config.unwrap_or_default();
        let compression_graph = build_graph(compaction_cfg.clone(), Arc::clone(&llm))?;
        let compress_node = Arc::new(CompressionGraphNode::new(compression_graph));

        let mut graph = StateGraph::<ReActState>::new();
        if let Some(s) = store {
            graph = graph.with_store(s);
        }
        let think_condition_path_map: HashMap<String, String> =
            [("tools".into(), "act".into()), (END.into(), END.into())]
                .into_iter()
                .collect();

        graph
            .add_node("think", Arc::new(think))
            .add_node("act", Arc::new(act))
            .add_node("observe", Arc::new(observe))
            .add_node("compress", compress_node)
            .add_edge(START, "think")
            .add_conditional_edges(
                "think",
                Arc::new(|state: &ReActState| tools_condition(state).as_str().to_string()),
                Some(think_condition_path_map),
            )
            .add_edge("act", "observe")
            .add_edge("observe", "compress")
            .add_edge("compress", "think");

        let graph = if verbose {
            graph.with_node_logging()
        } else {
            graph
        };

        let compiled = match (&checkpointer, verbose) {
            (Some(cp), true) => {
                let mw = Arc::new(LoggingNodeMiddleware::<ReActState>::default());
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

    pub async fn invoke(&self, user_message: &str) -> Result<ReActState, RunError> {
        self.invoke_with_config(user_message, None).await
    }

    pub async fn invoke_with_config(
        &self,
        user_message: &str,
        config: Option<RunnableConfig>,
    ) -> Result<ReActState, RunError> {
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_react_initial_state(
            user_message,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
            self.system_prompt.as_deref(),
        )
        .await?;
        let final_state = self.compiled.invoke(state, run_config).await?;
        Ok(final_state)
    }

    pub async fn stream_with_callback<F>(
        &self,
        user_message: &str,
        on_event: Option<F>,
    ) -> Result<ReActState, RunError>
    where
        F: FnMut(StreamEvent<ReActState>),
    {
        self.stream_with_config(user_message, None, on_event).await
    }

    pub async fn stream_with_config<F>(
        &self,
        user_message: &str,
        config: Option<RunnableConfig>,
        on_event: Option<F>,
    ) -> Result<ReActState, RunError>
    where
        F: FnMut(StreamEvent<ReActState>),
    {
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_react_initial_state(
            user_message,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
            self.system_prompt.as_deref(),
        )
        .await?;
        runner_common::run_stream_with_config(&self.compiled, state, run_config, on_event)
            .await
            .map_err(|_| RunError::StreamEndedWithoutState)
    }
}
