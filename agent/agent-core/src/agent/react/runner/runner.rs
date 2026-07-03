//! ReactRunner: compiled graph and stream.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Notify, OnceCell};

use crate::agent::react::REACT_SYSTEM_PROMPT;
use crate::compress::{build_graph, CompactionConfig, CompressionGraphNode};
use loom_graph_core::{
    CompilationError, CompiledStateGraph, LoggingNodeMiddleware, StateGraph, END, START,
};

use loom_llm::LlmProvider;
use loom_llm::message::UserContent;
use checkpoint::{Checkpointer, RunnableConfig, Store};
use crate::runner_common;
use crate::state::ReActState;
use stream_event::StreamEvent;
use tool_core::ToolRegistryLocked;
use checkpoint_sqlite_store::user_message::UserMessageStore;
use tool_core::active_operation::RunCancellation;

use super::error::RunError;
use super::initial_state::build_react_initial_state;
use super::options::{resolve_run_agent_options, AgentOptions};
use crate::agent::react::act_node::ActNode;

use crate::agent::react::observe_node::ObserveNode;
use crate::agent::react::title_assembly_middleware::TitleAssemblyMiddleware;
use crate::agent::react::title_node::TitleNode;
use crate::agent::react::think_node::ThinkNode;
use crate::agent::react::tools_condition;


pub struct ReactRunner {
    compiled: CompiledStateGraph<ReActState>,
    checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
    runnable_config: Option<RunnableConfig>,
    system_prompt: String,
    cancellation: Option<RunCancellation>,
    any_stream_event_sender: Option<Arc<dyn Fn(crate::run::TypedAnyStreamEvent) + Send + Sync>>,
    title_slot: Arc<OnceCell<String>>,
    title_notify: Arc<Notify>,
}

impl ReactRunner {
    pub fn with_cancellation(mut self, cancellation: Option<RunCancellation>) -> Self {
        self.cancellation = cancellation;
        self
    }

    /// Returns a clone of the runnable config used by this runner.
    /// Used to query the current `thread_id` and `checkpoint_id`.
    pub fn runnable_config(&self) -> Option<RunnableConfig> {
        self.runnable_config.clone()
    }

    /// Returns a reference to the checkpointer, if any.
    pub fn checkpointer(&self) -> Option<&Arc<dyn Checkpointer<ReActState>>> {
        self.checkpointer.as_ref()
    }

    /// Wait for the async title task to complete (with a timeout).
    /// Returns the title if it becomes available within the timeout.
    async fn wait_for_title(&self, timeout: Duration) -> Option<String> {
        if tokio::time::timeout(timeout, self.title_notify.notified())
            .await
            .is_err()
        {
            return None;
        }
        self.title_slot.get().cloned()
    }

    pub fn with_any_stream_event_sender(mut self, sender: Option<Arc<dyn Fn(crate::run::TypedAnyStreamEvent) + Send + Sync>>) -> Self {
        self.any_stream_event_sender = sender;
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
        any_stream_event_sender: Option<Arc<dyn Fn(crate::run::TypedAnyStreamEvent) + Send + Sync>>,
    ) -> Result<Self, CompilationError> {
        let think = ThinkNode::new(Arc::clone(&provider));
        let act = ActNode::new(tool_source)
            .with_run_cancellation(cancellation.clone())
            .with_any_stream_event_sender(any_stream_event_sender.clone());
        let observe = ObserveNode::with_loop();

        let compaction_cfg = compaction_config.unwrap_or_default();
        let compression_graph = build_graph(compaction_cfg.clone(), Arc::clone(&provider), None)?;
        let compress_node = Arc::new(CompressionGraphNode::new(compression_graph));

        let mut graph = StateGraph::<ReActState>::new();
        if let Some(s) = store {
            graph = graph.with_store(s);
        }

        let title_slot = Arc::new(OnceCell::new());
        let title_notify = Arc::new(Notify::new());
        let title_node = TitleNode::new(
            title_provider.unwrap_or_else(|| Arc::clone(&provider)),
            title_headers,
            Arc::clone(&title_slot),
            Arc::clone(&title_notify),
            any_stream_event_sender.clone(),
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
            .add_node("act", Arc::clone(&act) as Arc<dyn loom_graph_core::Node<ReActState>>)
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

        let graph = graph.with_metadata_extractor(Arc::new(|state: &ReActState| {
            state.summary.clone()
        }));

        let logger = if verbose {
            Some(Arc::new(LoggingNodeMiddleware::<ReActState>::default()))
        } else {
            None
        };
        let mw = Arc::new(TitleAssemblyMiddleware::new(Arc::clone(&title_slot), logger));

        let compiled = match &checkpointer {
            Some(cp) => graph.compile_with_checkpointer_and_middleware(Arc::clone(cp), mw)?,
            None => graph.compile_with_middleware(mw)?,
        };

        Ok(Self {
            compiled,
            checkpointer,
            runnable_config,
            system_prompt,
            cancellation,
            any_stream_event_sender,
            title_slot,
            title_notify,
        })
    }



    pub async fn stream_with_callback<F>(
        &self,
        user_message: impl Into<UserContent>,
        on_event: Option<F>,
    ) -> Result<runner_common::StreamRunOutcome<ReActState>, RunError>
    where
        F: Fn(StreamEvent<ReActState>) + Clone + Send + 'static,
    {
        self.stream_with_config(user_message, None, on_event).await
    }

    pub async fn stream_with_config<F>(
        &self,
        user_message: impl Into<UserContent>,
        config: Option<RunnableConfig>,
        on_event: Option<F>,
    ) -> Result<runner_common::StreamRunOutcome<ReActState>, RunError>
    where
        F: Fn(StreamEvent<ReActState>) + Clone + Send + 'static,
    {
        let user_content = user_message.into();
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_react_initial_state(
            &user_content,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
            &self.system_prompt,
        )
        .await?;
        let result = runner_common::run_stream_with_config(
            &self.compiled,
            state,
            run_config,
            on_event.clone(),
            self.cancellation.as_ref().map(RunCancellation::token),
        )
        .await;

        // After the graph finishes, wait briefly for the async title task.
        // If the title is ready, emit a __title_finalize__ sentinel so the
        // CLI can flush any pending_title that arrived during the run.
        if let Some(ref on_ev) = on_event {
            if let Some(title) = self.wait_for_title(Duration::from_secs(30)).await {
                let snapshot = ReActState {
                    summary: Some(title),
                    ..Default::default()
                };
                on_ev(StreamEvent::Updates {
                    node_id: "__title_finalize__".to_string(),
                    state: snapshot,
                    namespace: None,
                });
            }
        }

        match result {
            Ok(runner_common::StreamRunOutcome::Finished(s)) => Ok(runner_common::StreamRunOutcome::Finished(s)),
            Ok(runner_common::StreamRunOutcome::Cancelled) => Ok(runner_common::StreamRunOutcome::Cancelled),
            Err(runner_common::StreamRunError::Execution(err)) => Err(RunError::Execution(err)),
            Err(runner_common::StreamRunError::StreamEndedWithoutState(_)) => Err(RunError::StreamEndedWithoutState),
        }
    }
}

pub async fn run_react_graph_stream<F>(
    user_message: &str,
    options: Option<AgentOptions>,
    on_event: Option<F>,
) -> Result<runner_common::StreamRunOutcome<ReActState>, RunError>
where
    F: Fn(StreamEvent<ReActState>) + Clone + Send + 'static,
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
        None,
    )?;
    runner.stream_with_callback(user_message, on_event).await
}
