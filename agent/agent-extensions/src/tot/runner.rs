//! ToT graph runner: build, initial state, and stream.
//!
//! Graph: START → think_expand → think_evaluate → [tools_condition] → act | end,
//! act → observe → (observe returns Next::Node("think_expand")).

use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use agent::agent::react::{build_react_initial_state, REACT_SYSTEM_PROMPT};
use loom_graph::{CompilationError, CompiledStateGraph, LoggingNodeMiddleware};
use loom_memory::{CheckpointError, Checkpointer, RunnableConfig, Store};
use loom_llm::message::{Message, UserContent};
use agent::runner_common::{self, load_from_checkpoint_or_build};
use loom_stream::StreamEvent;
use tool_core::ToolRegistryLocked;
use loom_llm::LlmClient;
use loom_graph::{StateGraph, END, START};

use super::adapter_nodes::{TotActNode, TotObserveNode};
use super::backtrack_node::BacktrackNode;
use super::evaluate_node::ThinkEvaluateNode;
use super::expand_node::ThinkExpandNode;
use super::state::{TotExtension, TotState};

/// Condition for ToT graph: route based on state.core.tool_calls (chosen candidate applied).
fn tot_tools_condition(state: &TotState) -> &'static str {
    if state.core.tool_calls.is_empty() {
        END
    } else {
        "act"
    }
}

/// After observe: backtrack to next candidate if suggested and available, else think_expand.
fn tot_observe_condition(state: &TotState) -> &'static str {
    if state.tot.suggest_backtrack && state.tot.tried_indices.len() < state.tot.candidates.len() {
        "backtrack"
    } else {
        "think_expand"
    }
}

/// Builds the initial TotState for a run.
pub async fn build_tot_initial_state(
    user_message: &UserContent,
    checkpointer: Option<&dyn Checkpointer<TotState>>,
    runnable_config: Option<&RunnableConfig>,
    system_prompt: Option<&str>,
) -> Result<TotState, CheckpointError> {
    let system_prompt_owned = system_prompt.unwrap_or(REACT_SYSTEM_PROMPT).to_string();
    let user_message_owned = user_message.clone();
    load_from_checkpoint_or_build(
        checkpointer,
        runnable_config,
        user_message,
        async move {
            let core = build_react_initial_state(
                &user_message_owned,
                None,
                runnable_config,
                &system_prompt_owned,
            )
            .await?;
            Ok(TotState {
                core,
                tot: TotExtension::default(),
            })
        },
        |mut state, msg: UserContent| {
            state.core.messages.push(Message::user(msg));
            state.core.tool_calls = vec![];
            state.core.tool_results = vec![];
            state
        },
    )
    .await
}

pub use agent::RunnerError as TotRunError;

/// ToT graph runner: encapsulates compiled graph and persistence.
pub struct TotRunner {
    compiled: CompiledStateGraph<TotState>,
    checkpointer: Option<Arc<dyn Checkpointer<TotState>>>,
    runnable_config: Option<RunnableConfig>,
    system_prompt: Option<String>,
    cancellation: Option<CancellationToken>,
    any_stream_event_sender: Option<Arc<dyn Fn(loom_cli_types::AnyStreamEvent) + Send + Sync>>,
}

/// Wraps Arc<dyn LlmClient> to share one LLM between ThinkExpandNode and potential future nodes.
struct SharedLlm(Arc<dyn LlmClient>);

#[async_trait::async_trait]
impl LlmClient for SharedLlm {
    async fn invoke(
        &self,
        messages: &[loom_llm::message::Message],
    ) -> Result<loom_llm::LlmResponse, loom_llm::LlmError> {
        self.0.invoke(messages).await
    }
    async fn invoke_stream(
        &self,
        messages: &[loom_llm::message::Message],
        sink: Option<&dyn loom_llm::traits::StreamSink>,
        node_id: &str,
    ) -> Result<loom_llm::LlmResponse, loom_llm::LlmError> {
        self.0.invoke_stream(messages, sink, node_id).await
    }
}

impl TotRunner {
    pub fn with_cancellation(mut self, cancellation: Option<CancellationToken>) -> Self {
        self.cancellation = cancellation;
        self
    }

    pub fn with_any_stream_event_sender(mut self, sender: Option<Arc<dyn Fn(loom_cli_types::AnyStreamEvent) + Send + Sync>>) -> Self {
        self.any_stream_event_sender = sender;
        self
    }

    /// Creates a ToT runner with the given LLM, tool source, and optional persistence.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        llm: Arc<dyn LlmClient>,
        tool_source: Arc<ToolRegistryLocked>,
        checkpointer: Option<Arc<dyn Checkpointer<TotState>>>,
        store: Option<Arc<dyn Store>>,
        runnable_config: Option<RunnableConfig>,
        system_prompt: Option<String>,
        cancellation: Option<CancellationToken>,
        verbose: bool,
        max_depth: u32,
        candidates_per_step: u32,
        research_quality_addon: bool,
    ) -> Result<Self, CompilationError> {
        let expand = ThinkExpandNode::new(Box::new(SharedLlm(Arc::clone(&llm))))
            .with_candidates_per_step(candidates_per_step as usize)
            .with_research_quality_addon(research_quality_addon);
        let evaluate = ThinkEvaluateNode::new();
        let act = TotActNode::new(tool_source);
        let observe = TotObserveNode::new();
        let backtrack = BacktrackNode::new();

        let mut graph = StateGraph::<TotState>::new();
        if let Some(s) = store {
            graph = graph.with_store(s);
        }

        let eval_condition_path_map: HashMap<String, String> =
            [("act".into(), "act".into()), (END.into(), END.into())]
                .into_iter()
                .collect();

        let observe_condition_path_map: HashMap<String, String> = [
            ("backtrack".into(), "backtrack".into()),
            ("think_expand".into(), "think_expand".into()),
        ]
        .into_iter()
        .collect();

        graph
            .add_node("think_expand", Arc::new(expand))
            .add_node("think_evaluate", Arc::new(evaluate))
            .add_node("act", Arc::new(act))
            .add_node("observe", Arc::new(observe))
            .add_node("backtrack", Arc::new(backtrack))
            .add_edge(START, "think_expand")
            .add_edge("think_expand", "think_evaluate")
            .add_conditional_edges(
                "think_evaluate",
                Arc::new(|state: &TotState| tot_tools_condition(state).to_string()),
                Some(eval_condition_path_map),
            )
            .add_edge("act", "observe")
            .add_conditional_edges(
                "observe",
                Arc::new(|state: &TotState| tot_observe_condition(state).to_string()),
                Some(observe_condition_path_map),
            )
            .add_edge("backtrack", "act");

        let _ = max_depth; // reserved for backtrack / depth limit

        let graph = if verbose {
            graph.with_middleware(Arc::new(LoggingNodeMiddleware::<TotState>::default()))
        } else {
            graph
        };

        let compiled = match (&checkpointer, verbose) {
            (Some(cp), true) => {
                let mw = Arc::new(LoggingNodeMiddleware::<TotState>::default());
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
            any_stream_event_sender: None,
        })
    }

    /// Streams the graph execution; returns the final state.
    pub async fn stream_with_callback<F>(
        &self,
        user_message: impl Into<UserContent>,
        on_event: Option<F>,
    ) -> Result<runner_common::StreamRunOutcome<TotState>, TotRunError>
    where
        F: Fn(StreamEvent<TotState>) + Clone + Send + 'static,
    {
        self.stream_with_config(user_message, None, on_event).await
    }

    /// Streams with optional per-invoke config.
    pub async fn stream_with_config<F>(
        &self,
        user_message: impl Into<UserContent>,
        config: Option<RunnableConfig>,
        on_event: Option<F>,
    ) -> Result<runner_common::StreamRunOutcome<TotState>, TotRunError>
    where
        F: Fn(StreamEvent<TotState>) + Clone + Send + 'static,
    {
        let user_content = user_message.into();
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_tot_initial_state(
            &user_content,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
            self.system_prompt.as_deref(),
        )
        .await?;
        let result = runner_common::run_stream_with_config(
            &self.compiled,
            state,
            run_config,
            on_event,
            self.cancellation.clone(),
        )
        .await;
        match result {
            Ok(runner_common::StreamRunOutcome::Finished(s)) => Ok(runner_common::StreamRunOutcome::Finished(s)),
            Ok(runner_common::StreamRunOutcome::Cancelled) => Ok(runner_common::StreamRunOutcome::Cancelled),
            Err(runner_common::StreamRunError::Execution(err)) => Err(TotRunError::Execution(err)),
            Err(runner_common::StreamRunError::StreamEndedWithoutState(_)) => Err(TotRunError::StreamEndedWithoutState),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::state::TotCandidate;
    use super::*;
    use loom_llm::client::MockLlm;
    use loom_stream::StreamEvent;
    use loom_llm::ToolCall;
    use std::sync::{Arc, Mutex};

    fn state_with_tools(has_tools: bool) -> TotState {
        TotState {
            core: loom_stream::state::ReActState {
                tool_calls: if has_tools {
                    vec![ToolCall {
                        name: "search".to_string(),
                        arguments: "{}".to_string(),
                        id: None,
                    }]
                } else {
                    vec![]
                },
                ..loom_stream::state::ReActState::default()
            },
            tot: TotExtension::default(),
        }
    }

    #[test]
    fn tot_conditions_route_correctly() {
        assert_eq!(tot_tools_condition(&state_with_tools(false)), END);
        assert_eq!(tot_tools_condition(&state_with_tools(true)), "act");

        let mut s = state_with_tools(false);
        s.tot.suggest_backtrack = true;
        s.tot.candidates = vec![
            TotCandidate {
                thought: "a".to_string(),
                tool_calls: vec![],
                score: None,
            },
            TotCandidate {
                thought: "b".to_string(),
                tool_calls: vec![],
                score: None,
            },
        ];
        s.tot.tried_indices = vec![0];
        assert_eq!(tot_observe_condition(&s), "backtrack");
        s.tot.tried_indices = vec![0, 1];
        assert_eq!(tot_observe_condition(&s), "think_expand");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn build_tot_initial_state_builds_without_checkpoint() {
        let state = build_tot_initial_state(&loom_llm::message::UserContent::text("hello tot".to_string()), None, None, None)
            .await
            .unwrap();
        assert!(state.core.messages.len() >= 2);
        assert!(state.tot.candidates.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tot_runner_stream_with_mock_llm() {
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlm::with_no_tool_calls(
            "CANDIDATE 1: THOUGHT: answer directly | TOOL_CALLS: []",
        ));
        let runner = TotRunner::new(
            llm,
            tool_core::mock_registry(),
            None,
            None,
            None,
            None,
            None,
            false,
            3,
            3,
            false,
        )
        .unwrap();

        let events: Arc<Mutex<Vec<StreamEvent<TotState>>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let streamed = runner
            .stream_with_callback(
                "what is rust",
                Some(move |ev: StreamEvent<TotState>| {
                    events_clone.lock().unwrap().push(ev);
                }),
            )
            .await
            .unwrap();
        assert!(matches!(
            &streamed,
            agent::runner_common::StreamRunOutcome::Finished(s) if s.last_assistant_reply().is_some()
        ));
        assert!(!events.lock().unwrap().is_empty());
    }
}


