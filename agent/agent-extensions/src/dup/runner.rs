//! DUP graph runner: build, initial state, and stream.
//!
//! Graph: START → understand → plan → [tools_condition] → act | end, observe → plan.

use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use agent::agent::react::{build_react_initial_state, REACT_SYSTEM_PROMPT};

use loom_graph::{CompilationError, CompiledStateGraph, LoggingNodeMiddleware};
use loom_memory::{CheckpointError, Checkpointer, RunnableConfig, Store};
use loom_llm::message::Message;
use agent::runner_common::{self, load_from_checkpoint_or_build};
use loom_stream::StreamEvent;
use tool_core::ToolRegistryLocked;
use loom_llm::LlmClient;
use loom_graph::{StateGraph, END, START};

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
/// Uses load_from_checkpoint_or_build and build_react_initial_state for fresh core.
pub async fn build_dup_initial_state(
    user_message: &str,
    checkpointer: Option<&dyn Checkpointer<DupState>>,
    runnable_config: Option<&RunnableConfig>,
    system_prompt: Option<&str>,
) -> Result<DupState, CheckpointError> {
    let system_prompt_owned = system_prompt.unwrap_or(REACT_SYSTEM_PROMPT).to_string();
    let user_message_owned = user_message.to_string();
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
            Ok(DupState {
                core,
                understood: None,
            })
        },
        |mut state, msg| {
            state.core.messages.push(Message::user(msg));
            state.core.tool_calls = vec![];
            state.core.tool_results = vec![];
            state
        },
    )
    .await
}

pub use agent::RunnerError as DupRunError;

/// DUP graph runner: encapsulates compiled graph and persistence.
pub struct DupRunner {
    compiled: CompiledStateGraph<DupState>,
    checkpointer: Option<Arc<dyn Checkpointer<DupState>>>,
    runnable_config: Option<RunnableConfig>,
    system_prompt: Option<String>,
    cancellation: Option<CancellationToken>,
    any_stream_event_sender: Option<Arc<dyn Fn(loom_cli_types::AnyStreamEvent) + Send + Sync>>,
}

/// Wraps Arc<dyn LlmClient> to share one LLM between UnderstandNode and PlanNode.
struct SharedLlm(Arc<dyn LlmClient>);

#[async_trait::async_trait]
impl LlmClient for SharedLlm {
    async fn invoke(
        &self,
        messages: &[loom_llm::message::Message],
    ) -> Result<loom_llm::LlmResponse, loom_llm::error::AgentError> {
        self.0.invoke(messages).await
    }
    async fn invoke_stream(
        &self,
        messages: &[loom_llm::message::Message],
        tx: Option<tokio::sync::mpsc::Sender<loom_stream::MessageChunk>>,
    ) -> Result<loom_llm::LlmResponse, loom_llm::error::AgentError> {
        self.0.invoke_stream(messages, tx).await
    }
}

impl DupRunner {
    pub fn with_cancellation(mut self, cancellation: Option<CancellationToken>) -> Self {
        self.cancellation = cancellation;
        self
    }

    pub fn with_any_stream_event_sender(mut self, sender: Option<Arc<dyn Fn(loom_cli_types::AnyStreamEvent) + Send + Sync>>) -> Self {
        self.any_stream_event_sender = sender;
        self
    }

    /// Creates a DUP runner with the given LLM, tool source, and optional persistence.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        llm: Arc<dyn LlmClient>,
        tool_source: Arc<ToolRegistryLocked>,
        checkpointer: Option<Arc<dyn Checkpointer<DupState>>>,
        store: Option<Arc<dyn Store>>,
        runnable_config: Option<RunnableConfig>,
        system_prompt: Option<String>,
        cancellation: Option<CancellationToken>,
        verbose: bool,
    ) -> Result<Self, CompilationError> {
        let understand = UnderstandNode::new(Box::new(SharedLlm(Arc::clone(&llm))));
        let plan_provider: Arc<dyn loom_llm::LlmProvider> = Arc::new(loom_llm::client::FixedLlmProvider {
            client: Arc::clone(&llm),
            model_id: "dup".to_string(),
        });
        let plan = PlanNode::new(plan_provider);
        let act = DupActNode::new(tool_source);
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
            cancellation,
            any_stream_event_sender: None,
        })
    }

    /// Streams the graph execution; returns the final state.
    pub async fn stream_with_callback<F>(
        &self,
        user_message: &str,
        on_event: Option<F>,
    ) -> Result<runner_common::StreamRunOutcome<DupState>, DupRunError>
    where
        F: Fn(StreamEvent<DupState>) + Clone + Send + 'static,
    {
        self.stream_with_config(user_message, None, on_event).await
    }

    /// Streams with optional per-invoke config.
    pub async fn stream_with_config<F>(
        &self,
        user_message: &str,
        config: Option<RunnableConfig>,
        on_event: Option<F>,
    ) -> Result<runner_common::StreamRunOutcome<DupState>, DupRunError>
    where
        F: Fn(StreamEvent<DupState>) + Clone + Send + 'static,
    {
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_dup_initial_state(
            user_message,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
            self.system_prompt.as_deref(),
        )
        .await?;
        let event_forwarder: Option<std::sync::Arc<dyn Fn(StreamEvent<DupState>) + Send + Sync>> =
            self.any_stream_event_sender.as_ref().map(|sender| {
                let sender = sender.clone();
                std::sync::Arc::new(move |ev: StreamEvent<DupState>| {
                    if let Ok(json) = serde_json::to_value(&ev) {
                        sender(loom_cli_types::AnyStreamEvent::React(StreamEvent::Custom(json)));
                    }
                }) as std::sync::Arc<dyn Fn(StreamEvent<DupState>) + Send + Sync>
            });
        let result = runner_common::run_stream_with_config(
            &self.compiled,
            state,
            run_config,
            on_event,
            self.cancellation.clone(),
            event_forwarder,
        )
        .await;
        match result {
            Ok(runner_common::StreamRunOutcome::Finished(s)) => Ok(runner_common::StreamRunOutcome::Finished(s)),
            Ok(runner_common::StreamRunOutcome::Cancelled) => Ok(runner_common::StreamRunOutcome::Cancelled),
            Err(runner_common::StreamRunError::Execution(err)) => Err(DupRunError::Execution(err)),
            Err(runner_common::StreamRunError::StreamEndedWithoutState(_)) => Err(DupRunError::StreamEndedWithoutState),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_llm::client::MockLlm;
    use loom_stream::StreamEvent;
    use std::sync::{Arc, Mutex};

    #[test]
    fn dup_tools_condition_routes_correctly() {
        let no_tools = DupState {
            core: loom_types::state::ReActState::default(),
            understood: None,
        };
        assert_eq!(dup_tools_condition(&no_tools), END);

        let with_tools = DupState {
            core: loom_types::state::ReActState {
                tool_calls: vec![loom_types::state::ToolCall {
                    name: "x".to_string(),
                    arguments: "{}".to_string(),
                    id: None,
                }],
                ..loom_types::state::ReActState::default()
            },
            understood: None,
        };
        assert_eq!(dup_tools_condition(&with_tools), "act");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn build_dup_initial_state_builds_without_checkpoint() {
        let state = build_dup_initial_state("hello dup", None, None, None)
            .await
            .unwrap();
        assert!(state.understood.is_none());
        assert!(state.core.messages.len() >= 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dup_runner_stream_with_mock_llm() {
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlm::with_no_tool_calls("final answer"));
        let runner = DupRunner::new(
            llm,
            tool_core::mock_registry(),
            None,
            None,
            None,
            None,
            None,
            false,
        )
        .unwrap();

        let events: Arc<Mutex<Vec<StreamEvent<DupState>>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let streamed = runner
            .stream_with_callback(
                "what time is it?",
                Some(move |ev: StreamEvent<DupState>| {
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


