//! ReactRunner: compiled graph, invoke and stream.

use std::collections::HashMap;
use std::sync::Arc;

use crate::agent::react::REACT_SYSTEM_PROMPT;
use crate::compress::{build_graph, CompactionConfig, CompressionGraphNode};
use crate::graph::{
    CompilationError, CompiledStateGraph, LoggingNodeMiddleware, StateGraph, END, START,
};
use crate::helve::ApprovalPolicy;
use crate::llm::RetryLlmClient;
use crate::memory::{Checkpointer, RunnableConfig, Store};
use crate::runner_common;
use crate::state::ReActState;
use crate::stream::StreamEvent;
use crate::tool_source::ToolSource;
use crate::user_message::UserMessageStore;
use crate::{LlmClient, RunCancellation};
use crate::cli_run::AnyStreamEvent;

use super::error::RunError;
use super::initial_state::build_react_initial_state;
use super::options::TitleConfig;
use super::options::{resolve_run_agent_options, AgentOptions};
use crate::agent::react::act_node::{ActNode, HandleToolErrors};
use crate::agent::react::completion_check_node::CompletionCheckNode;
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
        llm: Box<dyn LlmClient>,
        tool_source: Box<dyn ToolSource>,
        checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
        store: Option<Arc<dyn Store>>,
        runnable_config: Option<RunnableConfig>,
        system_prompt: String,
        approval_policy: Option<ApprovalPolicy>,
        compaction_config: Option<CompactionConfig>,
        _user_message_store: Option<Arc<dyn UserMessageStore>>,
        cancellation: Option<RunCancellation>,
        verbose: bool,
        title_config: Option<TitleConfig>,
    ) -> Result<Self, CompilationError> {
        let llm: Arc<dyn LlmClient> = Arc::from(llm);
        let retry_llm: Arc<dyn LlmClient> = Arc::new(RetryLlmClient::new(llm.clone()));
        let think = ThinkNode::new(Arc::clone(&retry_llm));
        let act = ActNode::new(tool_source)
            .with_handle_tool_errors(HandleToolErrors::Always(None))
            .with_approval_policy(approval_policy);
        let observe = ObserveNode::with_loop();

        let compaction_cfg = compaction_config.unwrap_or_default();
        let compression_graph = build_graph(compaction_cfg.clone(), Arc::clone(&retry_llm), None)?;
        let compress_node = Arc::new(CompressionGraphNode::new(compression_graph));

        let mut graph = StateGraph::<ReActState>::new();
        if let Some(s) = store {
            graph = graph.with_store(s);
        }

        // Build graph with or without summarize node based on config
        let title_enabled = title_config.as_ref().is_some_and(|c| c.enabled);
        let completion_check_enabled = title_config
            .as_ref()
            .is_some_and(|c| c.enable_completion_check);

        if title_enabled {
            // Summarize node for generating session summaries after first think
            let title_node = TitleNode::new(Arc::clone(&retry_llm));

            if completion_check_enabled {
                let completion_check = CompletionCheckNode::new(Arc::clone(&retry_llm))
                    .with_max_iterations(10)
                    .with_message_window(5);

                let think_condition_path_map: HashMap<String, String> = [
                    ("title".into(), "title".into()),
                    ("tools".into(), "act".into()),
                    ("completion_check".into(), "completion_check".into()),
                ]
                .into_iter()
                .collect();

                let summarize_condition_path_map: HashMap<String, String> = [
                    ("tools".into(), "act".into()),
                    ("completion_check".into(), "completion_check".into()),
                ]
                .into_iter()
                .collect();

                let completion_check_path_map: HashMap<String, String> = [
                    ("continue".into(), "think".into()),
                    (END.into(), END.into()),
                ]
                .into_iter()
                .collect();

                graph
                    .add_node("think", Arc::new(think))
                    .add_node("title", Arc::new(title_node))
                    .add_node("completion_check", Arc::new(completion_check))
                    .add_node("act", Arc::new(act))
                    .add_node("observe", Arc::new(observe))
                    .add_node("compress", compress_node)
                    .add_edge(START, "think")
                    .add_conditional_edges(
                        "think",
                        Arc::new(|state: &ReActState| {
                            let user_msg_count = state
                                .messages
                                .iter()
                                .filter(|m| matches!(m, crate::message::Message::User(_)))
                                .count();
                            if user_msg_count == 1
                                && state.summary.is_none()
                                && state.think_count == 1
                            {
                                "title".to_string()
                            } else if !state.tool_calls.is_empty() {
                                "tools".to_string()
                            } else {
                                "completion_check".to_string()
                            }
                        }),
                        Some(think_condition_path_map),
                    )
                    .add_conditional_edges(
                        "title",
                        Arc::new(|state: &ReActState| {
                            if !state.tool_calls.is_empty() {
                                "tools".to_string()
                            } else {
                                "completion_check".to_string()
                            }
                        }),
                        Some(summarize_condition_path_map),
                    )
                    .add_conditional_edges(
                        "completion_check",
                        Arc::new(|state: &ReActState| {
                            if state.should_continue {
                                "continue".to_string()
                            } else {
                                END.to_string()
                            }
                        }),
                        Some(completion_check_path_map),
                    )
                    .add_edge("act", "observe")
                    .add_edge("observe", "compress")
                    .add_edge("compress", "think");
            } else {
                // No completion check: think/summarize -> tools or end
                let think_condition_path_map: HashMap<String, String> = [
                    ("title".into(), "title".into()),
                    ("tools".into(), "act".into()),
                    (END.into(), END.into()),
                ]
                .into_iter()
                .collect();

                let summarize_condition_path_map: HashMap<String, String> =
                    [("tools".into(), "act".into()), (END.into(), END.into())]
                        .into_iter()
                        .collect();

                graph
                    .add_node("think", Arc::new(think))
                    .add_node("title", Arc::new(title_node))
                    .add_node("act", Arc::new(act))
                    .add_node("observe", Arc::new(observe))
                    .add_node("compress", compress_node)
                    .add_edge(START, "think")
                    .add_conditional_edges(
                        "think",
                        Arc::new(|state: &ReActState| {
                            let user_msg_count = state
                                .messages
                                .iter()
                                .filter(|m| matches!(m, crate::message::Message::User(_)))
                                .count();
                            if user_msg_count == 1
                                && state.summary.is_none()
                                && state.think_count == 1
                            {
                                "title".to_string()
                            } else {
                                tools_condition(state).as_str().to_string()
                            }
                        }),
                        Some(think_condition_path_map),
                    )
                    .add_conditional_edges(
                        "title",
                        Arc::new(|state: &ReActState| tools_condition(state).as_str().to_string()),
                        Some(summarize_condition_path_map),
                    )
                    .add_edge("act", "observe")
                    .add_edge("observe", "compress")
                    .add_edge("compress", "think");
            }
        } else if completion_check_enabled {
            // No summarize, with completion check
            let completion_check = CompletionCheckNode::new(Arc::clone(&retry_llm))
                .with_max_iterations(10)
                .with_message_window(5);

            let think_condition_path_map: HashMap<String, String> = [
                ("tools".into(), "act".into()),
                ("completion_check".into(), "completion_check".into()),
                (END.into(), END.into()),
            ]
            .into_iter()
            .collect();

            let completion_check_path_map: HashMap<String, String> = [
                ("continue".into(), "think".into()),
                (END.into(), END.into()),
            ]
            .into_iter()
            .collect();

            graph
                .add_node("think", Arc::new(think))
                .add_node("completion_check", Arc::new(completion_check))
                .add_node("act", Arc::new(act))
                .add_node("observe", Arc::new(observe))
                .add_node("compress", compress_node)
                .add_edge(START, "think")
                .add_conditional_edges(
                    "think",
                    Arc::new(|state: &ReActState| {
                        if !state.tool_calls.is_empty() {
                            "tools".to_string()
                        } else {
                            "completion_check".to_string()
                        }
                    }),
                    Some(think_condition_path_map),
                )
                .add_conditional_edges(
                    "completion_check",
                    Arc::new(|state: &ReActState| {
                        if state.should_continue {
                            "continue".to_string()
                        } else {
                            END.to_string()
                        }
                    }),
                    Some(completion_check_path_map),
                )
                .add_edge("act", "observe")
                .add_edge("observe", "compress")
                .add_edge("compress", "think");
        } else {
            // No summarize, no completion check - original graph
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
        }

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
            cancellation,
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
            &self.system_prompt,
        )
        .await?;
        let final_state = self.compiled.invoke(state, run_config).await?;
        Ok(final_state)
    }

    pub async fn stream_with_callback<F>(
        &self,
        user_message: &str,
        on_event: Option<F>,
    ) -> Result<runner_common::StreamRunOutcome<ReActState>, RunError>
    where
        F: FnMut(StreamEvent<ReActState>),
    {
        self.stream_with_config(user_message, None, on_event, None).await
    }

    pub async fn stream_with_config<F>(
        &self,
        user_message: &str,
        config: Option<RunnableConfig>,
        on_event: Option<F>,
        any_stream_event_sender: Option<Arc<dyn Fn(AnyStreamEvent) + Send + Sync>>,
    ) -> Result<runner_common::StreamRunOutcome<ReActState>, RunError>
    where
        F: FnMut(StreamEvent<ReActState>),
    {
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_react_initial_state(
            user_message,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
            &self.system_prompt,
        )
        .await?;
        runner_common::run_stream_with_config(
            &self.compiled,
            state,
            run_config,
            on_event,
            self.cancellation.as_ref().map(RunCancellation::token),
            self.cancellation.clone(),
            any_stream_event_sender,
        )
        .await
        .map_err(|e| match e {
            runner_common::StreamRunError::Execution(err) => RunError::Execution(err),
            runner_common::StreamRunError::StreamEndedWithoutState(_) => {
                RunError::StreamEndedWithoutState
            }
        })
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
        REACT_SYSTEM_PROMPT.to_string(),
        None,
        None,
        opts.user_message_store,
        None,
        opts.verbose,
        Some(opts.title_config),
    )?;
    runner.invoke(user_message).await
}

pub async fn run_react_graph_stream<F>(
    user_message: &str,
    options: Option<AgentOptions>,
    on_event: Option<F>,
) -> Result<runner_common::StreamRunOutcome<ReActState>, RunError>
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
        REACT_SYSTEM_PROMPT.to_string(),
        None,
        None,
        opts.user_message_store,
        None,
        opts.verbose,
        Some(opts.title_config),
    )?;
    runner.stream_with_callback(user_message, on_event).await
}
