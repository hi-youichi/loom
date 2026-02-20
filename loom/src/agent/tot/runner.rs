//! ToT graph runner: build, initial state, invoke and stream.
//!
//! Graph: START → think_expand → think_evaluate → [tools_condition] → act | end,
//! act → observe → (observe returns Next::Node("think_expand")).

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::AgentError;
use crate::graph::{CompilationError, CompiledStateGraph, LoggingNodeMiddleware};
use crate::helve::ApprovalPolicy;
use crate::memory::{CheckpointError, Checkpointer, RunnableConfig, Store};
use crate::message::Message;
use crate::agent::react::{build_react_initial_state, REACT_SYSTEM_PROMPT};
use crate::runner_common::{self, load_from_checkpoint_or_build};
use crate::stream::StreamEvent;
use crate::tool_source::ToolSource;
use crate::LlmClient;
use crate::{StateGraph, END, START};

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
    user_message: &str,
    checkpointer: Option<&dyn Checkpointer<TotState>>,
    runnable_config: Option<&RunnableConfig>,
    system_prompt: Option<&str>,
) -> Result<TotState, CheckpointError> {
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
                Some(&system_prompt_owned),
            )
            .await?;
            Ok(TotState {
                core,
                tot: TotExtension::default(),
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

/// Error type for TotRunner operations.
#[derive(Debug, thiserror::Error)]
pub enum TotRunError {
    #[error("compilation failed: {0}")]
    Compilation(#[from] CompilationError),
    #[error("checkpoint error: {0}")]
    Checkpoint(#[from] CheckpointError),
    #[error("execution failed: {0}")]
    Execution(#[from] AgentError),
    #[error("stream ended without final state")]
    StreamEndedWithoutState,
}

/// ToT graph runner: encapsulates compiled graph and persistence.
pub struct TotRunner {
    compiled: CompiledStateGraph<TotState>,
    checkpointer: Option<Arc<dyn Checkpointer<TotState>>>,
    runnable_config: Option<RunnableConfig>,
    system_prompt: Option<String>,
}

/// Wraps Arc<dyn LlmClient> to share one LLM between ThinkExpandNode and potential future nodes.
struct SharedLlm(Arc<dyn LlmClient>);

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

impl TotRunner {
    /// Creates a ToT runner with the given LLM, tool source, and optional persistence.
    pub fn new(
        llm: Arc<dyn LlmClient>,
        tool_source: Box<dyn ToolSource>,
        checkpointer: Option<Arc<dyn Checkpointer<TotState>>>,
        store: Option<Arc<dyn Store>>,
        runnable_config: Option<RunnableConfig>,
        system_prompt: Option<String>,
        approval_policy: Option<ApprovalPolicy>,
        verbose: bool,
        max_depth: u32,
        candidates_per_step: u32,
        research_quality_addon: bool,
    ) -> Result<Self, CompilationError> {
        let expand = ThinkExpandNode::new(Box::new(SharedLlm(Arc::clone(&llm))))
            .with_candidates_per_step(candidates_per_step as usize)
            .with_research_quality_addon(research_quality_addon);
        let evaluate = ThinkEvaluateNode::new();
        let act = TotActNode::new(tool_source).with_approval_policy(approval_policy);
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
        })
    }

    /// Invokes the graph with the given user message.
    pub async fn invoke(&self, user_message: &str) -> Result<TotState, TotRunError> {
        self.invoke_with_config(user_message, None).await
    }

    /// Invokes with optional per-invoke config.
    pub async fn invoke_with_config(
        &self,
        user_message: &str,
        config: Option<RunnableConfig>,
    ) -> Result<TotState, TotRunError> {
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_tot_initial_state(
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
    ) -> Result<TotState, TotRunError>
    where
        F: FnMut(StreamEvent<TotState>),
    {
        self.stream_with_config(user_message, None, on_event).await
    }

    /// Streams with optional per-invoke config.
    pub async fn stream_with_config<F>(
        &self,
        user_message: &str,
        config: Option<RunnableConfig>,
        on_event: Option<F>,
    ) -> Result<TotState, TotRunError>
    where
        F: FnMut(StreamEvent<TotState>),
    {
        let run_config = config.or_else(|| self.runnable_config.clone());
        let state = build_tot_initial_state(
            user_message,
            self.checkpointer.as_deref(),
            run_config.as_ref(),
            self.system_prompt.as_deref(),
        )
        .await?;
        runner_common::run_stream_with_config(&self.compiled, state, run_config, on_event)
            .await
            .map_err(|_| TotRunError::StreamEndedWithoutState)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MockLlm, MockToolSource, StreamEvent, ToolCall};
    use super::super::state::TotCandidate;
    use std::sync::{Arc, Mutex};

    fn state_with_tools(has_tools: bool) -> TotState {
        TotState {
            core: crate::ReActState {
                tool_calls: if has_tools {
                    vec![ToolCall {
                        name: "search".to_string(),
                        arguments: "{}".to_string(),
                        id: None,
                    }]
                } else {
                    vec![]
                },
                ..crate::ReActState::default()
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

    #[tokio::test]
    async fn build_tot_initial_state_builds_without_checkpoint() {
        let state = build_tot_initial_state("hello tot", None, None, None)
            .await
            .unwrap();
        assert!(state.core.messages.len() >= 2);
        assert!(state.tot.candidates.is_empty());
    }

    #[tokio::test]
    async fn tot_runner_invoke_and_stream_with_mock_llm() {
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlm::with_no_tool_calls(
            "CANDIDATE 1: THOUGHT: answer directly | TOOL_CALLS: []",
        ));
        let runner = TotRunner::new(
            llm,
            Box::new(MockToolSource::get_time_example()),
            None,
            None,
            None,
            None,
            None,
            false,
            3,
            2,
            false,
        )
        .unwrap();

        let out = runner.invoke("what is rust").await.unwrap();
        assert!(out.last_assistant_reply().is_some());

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
        assert!(streamed.last_assistant_reply().is_some());
        assert!(!events.lock().unwrap().is_empty());
    }
}
