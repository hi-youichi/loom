//! ReactRunner: compiled graph, invoke and stream.

use std::collections::HashMap;
use std::sync::Arc;

use crate::compress::{build_graph, CompactionConfig, CompressionGraphNode};
use crate::graph::{
    CompilationError, CompiledStateGraph, LoggingNodeMiddleware, StateGraph, END, START,
};
use crate::helve::ApprovalPolicy;
use crate::memory::{Checkpointer, RunnableConfig, Store};
use crate::runner_common;
use crate::state::ReActState;
use crate::stream::StreamEvent;
use crate::tool_source::ToolSource;
use crate::user_message::UserMessageStore;
use crate::LlmClient;

use super::error::RunError;
use super::initial_state::build_react_initial_state;
use super::options::{resolve_run_agent_options, AgentOptions};
use crate::agent::react::act_node::{ActNode, HandleToolErrors};
use crate::agent::react::observe_node::ObserveNode;
use crate::agent::react::think_node::ThinkNode;
use crate::agent::react::tools_condition;
use crate::agent::react::with_node_logging::WithNodeLogging;

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
        _user_message_store: Option<Arc<dyn UserMessageStore>>,
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
        opts.user_message_store,
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
        opts.user_message_store,
        opts.verbose,
    )?;
    runner.stream_with_callback(user_message, on_event).await
}
