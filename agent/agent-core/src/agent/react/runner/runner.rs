//! ReactRunner: compiled graph and stream.

use std::collections::HashMap;
use std::sync::Arc;

use crate::agent::react::REACT_SYSTEM_PROMPT;
use loom_compress::{build_graph, CompactionConfig, CompressionGraphNode};
use loom_graph::{
    CompilationError, CompiledStateGraph, LoggingNodeMiddleware, StateGraph, END, START,
};

use loom_llm::LlmProvider;
use loom_memory::{Checkpointer, RunnableConfig, Store};
use crate::runner_common;
use loom_types::state::ReActState;
use loom_stream::StreamEvent;
use tool_core::ToolRegistryLocked;
use loom_memory::user_message::UserMessageStore;
use loom_types::active_operation::RunCancellation;

use super::error::RunError;
use super::initial_state::build_react_initial_state;
use super::options::{resolve_run_agent_options, AgentOptions};
use crate::agent::react::act_node::ActNode;

use crate::agent::react::observe_node::ObserveNode;
use crate::agent::react::title_node::TitleNode;
use crate::agent::react::think_node::ThinkNode;
use crate::agent::react::tools_condition;
use crate::agent::react::with_node_logging::WithNodeLogging;

pub struct ReactRunner {
    compiled: CompiledStateGraph<ReActState>,
    checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
    runnable_config: Option<RunnableConfig>,
    system_prompt: String,
    cancellation: Option<RunCancellation>,
}

impl ReactRunner {
    pub fn with_cancellation(mut self, cancellation: Option<RunCancellation>) -> Self {
        self.cancellation = cancellation;
        self
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        tool_source: Arc<ToolRegistryLocked>,
        checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
        store: Option<Arc<dyn Store>>,
        runnable_config: Option<RunnableConfig>,
        system_prompt: String,
        compaction_config: Option<CompactionConfig>,
        _user_message_store: Option<Arc<dyn UserMessageStore>>,
        cancellation: Option<RunCancellation>,
        verbose: bool,
        title_provider: Option<Arc<dyn LlmProvider>>,
        title_headers: Option<loom_llm::LlmHeaders>,
    ) -> Result<Self, CompilationError> {
        let think = ThinkNode::new(Arc::clone(&provider));
        let act = ActNode::new(tool_source)
            .with_run_cancellation(cancellation.clone());
        let observe = ObserveNode::with_loop();

        let compaction_cfg = compaction_config.unwrap_or_default();
        let compression_graph = build_graph(compaction_cfg.clone(), Arc::clone(&provider), None)?;
        let compress_node = Arc::new(CompressionGraphNode::new(compression_graph));

        let mut graph = StateGraph::<ReActState>::new();
        if let Some(s) = store {
            graph = graph.with_store(s);
        }

        let title_node = TitleNode::new(
            title_provider.unwrap_or_else(|| Arc::clone(&provider)),
            title_headers,
        );

        let think_condition_path_map: HashMap<String, String> = [
            ("tools".into(), "act".into()),
            (END.into(), END.into()),
        ]
        .into_iter()
        .collect();

        let act = Arc::new(act);

        graph
            .add_node("think", Arc::new(think))
            .add_node("title", Arc::new(title_node))
            .add_node("act", Arc::clone(&act) as Arc<dyn loom_graph::Node<ReActState>>)
            .add_node("observe", Arc::new(observe))
            .add_node("compress", compress_node)
            .add_edge(START, "title")
            .add_edge("title", "think")
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

        let graph = graph.with_metadata_extractor(Arc::new(|state: &ReActState| {
            state.summary.clone()
        }));

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
            cancellation,
        })
    }



    pub async fn stream_with_callback<F>(
        &self,
        user_message: &str,
        on_event: Option<F>,
    ) -> Result<runner_common::StreamRunOutcome<ReActState, runner_common::StreamRunError>, RunError>
    where
        F: FnMut(StreamEvent<ReActState>) + Send + 'static,
    {
        self.stream_with_config(user_message, None, on_event).await
    }

    pub async fn stream_with_config<F>(
        &self,
        user_message: &str,
        config: Option<RunnableConfig>,
        on_event: Option<F>,
    ) -> Result<runner_common::StreamRunOutcome<ReActState, runner_common::StreamRunError>, RunError>
    where
        F: FnMut(StreamEvent<ReActState>) + Send + 'static,
    {
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_react_initial_state(
            user_message,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
            &self.system_prompt,
        )
        .await?;
        let result = runner_common::run_stream_with_config(
            self.compiled.clone(),
            state,
            run_config,
            on_event,
            self.cancellation.as_ref().map(RunCancellation::token),
            None,
        )
        .await;
        match result {
            Ok(outcome) => match outcome {
                runner_common::StreamRunOutcome::Completed(s) => Ok(runner_common::StreamRunOutcome::Completed(s)),
                runner_common::StreamRunOutcome::Cancelled => Ok(runner_common::StreamRunOutcome::Cancelled),
                runner_common::StreamRunOutcome::Error(runner_common::StreamRunError::Execution(err)) => Err(RunError::Execution(err)),
                runner_common::StreamRunOutcome::Empty => Ok(runner_common::StreamRunOutcome::Empty),
            },
            Err(runner_common::StreamRunError::Execution(err)) => Err(RunError::Execution(err)),
        }
    }
}

pub async fn run_react_graph_stream<F>(
    user_message: &str,
    options: Option<AgentOptions>,
    on_event: Option<F>,
) -> Result<runner_common::StreamRunOutcome<ReActState, runner_common::StreamRunError>, RunError>
where
    F: FnMut(StreamEvent<ReActState>) + Send + 'static,
{
    let opts = resolve_run_agent_options(options.unwrap_or_default());
    let runner = ReactRunner::new(
        opts.provider,
        opts.tool_source,
        opts.checkpointer,
        opts.store,
        opts.runnable_config,
        REACT_SYSTEM_PROMPT.to_string(),
        None,
        opts.user_message_store,
        None,
        opts.verbose,
        None,
        None,
    )?;
    runner.stream_with_callback(user_message, on_event).await
}
