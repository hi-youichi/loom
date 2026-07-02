//! GoT graph runner: build, initial state, and stream.
//!
//! Graph: START → plan_graph → execute_graph → [has_pending] → execute_graph | END.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;


use loom_graph_core::{CompilationError, CompiledStateGraph, LoggingNodeMiddleware};
use checkpoint::{CheckpointError, Checkpointer, RunnableConfig, Store};
use crate::runner_common;
use stream_event::StreamEvent;
use tool_core::ToolRegistryLocked;
use loom_llm::LlmClient;
use loom_graph_core::{StateGraph, END, START};
use loom_llm::message::UserContent;

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
    user_message: &UserContent,
    checkpointer: Option<&dyn Checkpointer<GotState>>,
    runnable_config: Option<&RunnableConfig>,
) -> Result<GotState, CheckpointError> {
    let input_text = user_message.as_text().into_owned();
    if let (Some(cp), Some(config)) = (checkpointer, runnable_config) {
        if config.thread_id.is_some() {
            let tuple = cp.get_tuple(config).await?;
            if let Some((checkpoint, _)) = tuple {
                let mut state = checkpoint.channel_values.clone();
                state.input_message = input_text;
                return Ok(state);
            }
        }
    }

    Ok(GotState {
        input_message: input_text,
        task_graph: super::state::TaskGraph::default(),
        node_states: std::collections::HashMap::new(),
    })
}

pub use crate::RunnerError as GotRunError;

/// GoT graph runner: encapsulates compiled graph and optional persistence.
pub struct GotRunner {
    compiled: CompiledStateGraph<GotState>,
    checkpointer: Option<Arc<dyn Checkpointer<GotState>>>,
    runnable_config: Option<RunnableConfig>,
    cancellation: Option<CancellationToken>,
    any_stream_event_sender: Option<Arc<dyn Fn(crate::run::TypedAnyStreamEvent) + Send + Sync>>,
}

impl GotRunner {
    pub fn with_cancellation(mut self, cancellation: Option<CancellationToken>) -> Self {
        self.cancellation = cancellation;
        self
    }

    pub fn with_any_stream_event_sender(mut self, sender: Option<Arc<dyn Fn(crate::run::TypedAnyStreamEvent) + Send + Sync>>) -> Self {
        self.any_stream_event_sender = sender;
        self
    }

    /// Creates a GoT runner with the given LLM, tool source, and optional persistence.
    /// When `adaptive` is true, enables AGoT: complex nodes may be expanded into subgraphs
    /// after completion.
    /// When `agot_llm_complexity` is true, use LLM to decide simple vs complex instead of heuristic.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        llm: Arc<dyn LlmClient>,
        tool_source: Arc<ToolRegistryLocked>,
        checkpointer: Option<Arc<dyn Checkpointer<GotState>>>,
        store: Option<Arc<dyn Store>>,
        runnable_config: Option<RunnableConfig>,
        cancellation: Option<CancellationToken>,
        verbose: bool,
        adaptive: bool,
        agot_llm_complexity: bool,
    ) -> Result<Self, CompilationError> {
        let plan = PlanGraphNode::new(Box::new(SharedLlm(Arc::clone(&llm))));
        let provider: Arc<dyn loom_llm::LlmProvider> = Arc::new(loom_llm::client::FixedLlmProvider {
            client: Arc::clone(&llm),
            model_id: "got".to_string(),
        });
        let execute = ExecuteGraphNode::new(provider, tool_source, adaptive, agot_llm_complexity);

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
            cancellation,
            any_stream_event_sender: None,
        })
    }

    /// Streams the graph execution; returns the final state.
    pub async fn stream_with_callback<F>(
        &self,
        user_message: impl Into<UserContent>,
        on_event: Option<F>,
    ) -> Result<runner_common::StreamRunOutcome<GotState>, GotRunError>
    where
        F: Fn(StreamEvent<GotState>) + Clone + Send + 'static,
    {
        self.stream_with_config(user_message, None, on_event).await
    }

    /// Streams with optional per-invoke config.
    pub async fn stream_with_config<F>(
        &self,
        user_message: impl Into<UserContent>,
        config: Option<RunnableConfig>,
        on_event: Option<F>,
    ) -> Result<runner_common::StreamRunOutcome<GotState>, GotRunError>
    where
        F: Fn(StreamEvent<GotState>) + Clone + Send + 'static,
    {
        let user_content = user_message.into();
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_got_initial_state(
            &user_content,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
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
            Err(runner_common::StreamRunError::Execution(err)) => Err(GotRunError::Execution(err)),
            Err(runner_common::StreamRunError::StreamEndedWithoutState(_)) => Err(GotRunError::StreamEndedWithoutState),
        }
    }
}

/// Wraps Arc<dyn LlmClient> to share one LLM between PlanGraphNode and ExecuteGraphNode.
pub(super) struct SharedLlm(Arc<dyn LlmClient>);

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

#[cfg(test)]
mod tests {
    use super::*;
    use loom_llm::client::MockLlm;
    use stream_event::StreamEvent;
    use std::sync::{Arc, Mutex};

    #[test]
    fn got_execute_condition_routes_based_on_ready_nodes() {
        let empty = GotState::default();
        assert_eq!(got_execute_condition(&empty), END);

        let state = GotState {
            input_message: "q".to_string(),
            task_graph: super::super::state::TaskGraph {
                nodes: vec![super::super::state::TaskNode {
                    id: "n1".to_string(),
                    description: "step".to_string(),
                    tool_calls: vec![],
                }],
                edges: vec![],
            },
            node_states: [(
                "n1".into(),
                super::super::state::TaskNodeState::default(),
            )]
            .into_iter()
            .collect(),
        };
        assert_eq!(got_execute_condition(&state), "execute_graph");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn build_got_initial_state_without_checkpoint_uses_input_message() {
        let state = build_got_initial_state(&loom_llm::message::UserContent::text("hello got".to_string()), None, None)
            .await
            .unwrap();
        assert_eq!(state.input_message, "hello got");
        assert!(state.task_graph.nodes.is_empty());
        assert!(state.node_states.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn got_runner_stream_with_mock_llm() {
        let llm_response =
            r#"{\"nodes\":[{\"id\":\"collect\",\"description\":\"collect info\"}],\"edges\":[]}"#;
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlm::with_no_tool_calls(llm_response));
        let runner = GotRunner::new(
            llm,
            tool_core::mock_registry(),
            None,
            None,
            None,
            None,
            false,
            false,
            false,
        )
        .unwrap();

        let events: Arc<Mutex<Vec<StreamEvent<GotState>>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let streamed = runner
            .stream_with_callback(
                "help me",
                Some(move |ev: StreamEvent<GotState>| {
                    events_clone.lock().unwrap().push(ev);
                }),
            )
            .await
            .unwrap();
        assert!(matches!(
            &streamed,
            crate::runner_common::StreamRunOutcome::Finished(s) if !s.task_graph.nodes.is_empty()
        ));
        assert!(!events.lock().unwrap().is_empty());
    }
}


