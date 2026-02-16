//! Compiled state graph: immutable, supports invoke only.
//!
//! Built by `StateGraph::compile` or `compile_with_checkpointer`. Holds nodes and
//! edge order (derived from explicit edges at compile time), optional checkpointer.
//! When checkpointer is set and config.thread_id is provided, final state is saved after invoke.

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::channels::BoxedStateUpdater;
use crate::error::AgentError;
use crate::memory::{Checkpoint, CheckpointSource, Checkpointer, RunnableConfig, Store};
use crate::stream::{StreamEvent, StreamMode};

use super::interrupt::InterruptHandler;
use super::logging::{
    log_graph_complete, log_graph_error, log_graph_start, log_node_complete, log_node_start,
    log_node_state, log_state_update,
};
use super::node_middleware::NodeMiddleware;
use super::retry::RetryPolicy;
use super::state_graph::END;
use super::{Next, NextEntry, Node, RunContext};

/// Compiled graph: immutable structure, supports invoke only.
///
/// Created by `StateGraph::compile()` or `compile_with_checkpointer()`. Runs from first node;
/// uses each node's returned `Next` or conditional router (when present) to choose next node.
/// When checkpointer is set, invoke(state, config) saves the final state for config.thread_id.
/// When store is set (via `with_store` before compile), nodes can use it for long-term memory.
#[derive(Clone)]
pub struct CompiledStateGraph<S> {
    pub(super) nodes: HashMap<String, Arc<dyn Node<S>>>,
    /// First node to run (from START). Used when no next_map or for initial step.
    pub(super) first_node_id: String,
    /// Linear order of nodes (used for Next::Continue when no conditional). Empty when graph has conditional edges.
    pub(super) edge_order: Vec<String>,
    /// Map from node id to how to get next: Unconditional(to_id) or Conditional(router). Used for routing after each node.
    pub(super) next_map: HashMap<String, NextEntry<S>>,
    pub(super) checkpointer: Option<Arc<dyn Checkpointer<S>>>,
    /// Optional long-term store; set when graph was built with `with_store`. Nodes use it via config or construction.
    pub(super) store: Option<Arc<dyn Store>>,
    /// Optional node middleware; set when built with `compile_with_middleware` or `compile_with_checkpointer_and_middleware`.
    pub(super) middleware: Option<Arc<dyn NodeMiddleware<S>>>,
    /// State updater that controls how node outputs are merged into state.
    /// Default is `ReplaceUpdater` which fully replaces the state.
    pub(super) state_updater: BoxedStateUpdater<S>,
    /// Retry policy for node execution. Default is `RetryPolicy::None`.
    pub(super) retry_policy: RetryPolicy,
    /// Optional interrupt handler for human-in-the-loop scenarios.
    pub(super) interrupt_handler: Option<Arc<dyn InterruptHandler>>,
}

impl<S> CompiledStateGraph<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Execute a node with retry logic.
    ///
    /// Attempts to run the node, retrying according to the configured retry policy
    /// if the execution fails.
    async fn execute_node_with_retry(
        &self,
        node: Arc<dyn Node<S>>,
        state: S,
        run_ctx: Option<&RunContext<S>>,
    ) -> Result<(S, Next), AgentError> {
        let mut attempt = 0;
        loop {
            let current_state = state.clone();
            let result = if let Some(middleware) = &self.middleware {
                let node_id = node.id().to_string();
                let run_ctx_owned = run_ctx.cloned();
                let node_clone = node.clone();
                middleware
                    .around_run(
                        &node_id,
                        current_state,
                        Box::new(move |s| {
                            let node = node_clone.clone();
                            let run_ctx_inner = run_ctx_owned.clone();
                            Box::pin(async move {
                                if let Some(ctx) = run_ctx_inner.as_ref() {
                                    node.run_with_context(s, ctx).await
                                } else {
                                    node.run(s).await
                                }
                            })
                        }),
                    )
                    .await
            } else if let Some(ctx) = run_ctx {
                node.run_with_context(current_state, ctx).await
            } else {
                node.run(current_state).await
            };

            match result {
                Ok(output) => return Ok(output),
                Err(e) => {
                    // Check if we should retry
                    if self.retry_policy.should_retry(attempt) {
                        let delay = self.retry_policy.delay(attempt);
                        if delay > std::time::Duration::ZERO {
                            tokio::time::sleep(delay).await;
                        }
                        attempt += 1;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }

    /// Shared run loop used by invoke() and stream(): steps through nodes until completion.
    ///
    /// This method includes:
    /// - Structured logging for graph execution events
    /// - Retry mechanism for transient failures
    /// - Interrupt handling support
    async fn run_loop_inner(
        &self,
        state: &mut S,
        config: &Option<RunnableConfig>,
        current_id: &mut String,
        run_ctx: Option<&RunContext<S>>,
    ) -> Result<(), AgentError> {
        log_graph_start();

        loop {
            let node = self
                .nodes
                .get(current_id)
                .expect("compiled graph has all nodes")
                .clone();
            let current_state = state.clone();

            // Log node execution start and current state
            log_node_start(current_id);
            log_node_state(current_id, &current_state);

            // Emit TaskStart event if Tasks or Debug mode is enabled
            if let Some(ctx) = run_ctx {
                if let Some(tx) = &ctx.stream_tx {
                    if ctx.stream_mode.contains(&StreamMode::Tasks)
                        || ctx.stream_mode.contains(&StreamMode::Debug)
                    {
                        let _ = tx
                            .send(StreamEvent::TaskStart {
                                node_id: current_id.clone(),
                            })
                            .await;
                    }
                }
            }

            // Execute node with retry logic
            let result = self
                .execute_node_with_retry(node, current_state, run_ctx)
                .await;

            // Handle errors (including interrupts)
            let (new_state, next) = match result {
                Ok(output) => output,
                Err(AgentError::Interrupted(ref interrupt)) => {
                    // Handle interrupt: save checkpoint and optionally call handler
                    if let (Some(cp), Some(cfg)) = (&self.checkpointer, config) {
                        if cfg.thread_id.is_some() {
                            // Save checkpoint before interrupt so we can resume later
                            let checkpoint =
                                Checkpoint::from_state(state.clone(), CheckpointSource::Update, 0);
                            let _ = cp.put(cfg, &checkpoint).await;

                            // Emit checkpoint event if enabled
                            if let Some(ctx) = run_ctx {
                                if let Some(tx) = &ctx.stream_tx {
                                    if ctx.stream_mode.contains(&StreamMode::Checkpoints)
                                        || ctx.stream_mode.contains(&StreamMode::Debug)
                                    {
                                        let checkpoint_ns = if cfg.checkpoint_ns.is_empty() {
                                            None
                                        } else {
                                            Some(cfg.checkpoint_ns.clone())
                                        };
                                        let _ = tx
                                            .send(StreamEvent::Checkpoint(
                                                crate::stream::CheckpointEvent {
                                                    checkpoint_id: checkpoint.id.clone(),
                                                    timestamp: checkpoint.ts.clone(),
                                                    step: checkpoint.metadata.step,
                                                    state: state.clone(),
                                                    thread_id: cfg.thread_id.clone(),
                                                    checkpoint_ns,
                                                },
                                            ))
                                            .await;
                                    }
                                }
                            }
                        }
                    }

                    // Call interrupt handler if configured
                    if let Some(handler) = &self.interrupt_handler {
                        let _ = handler.handle_interrupt(&interrupt.0);
                    }

                    // Emit TaskEnd with interrupt info
                    if let Some(ctx) = run_ctx {
                        if let Some(tx) = &ctx.stream_tx {
                            if ctx.stream_mode.contains(&StreamMode::Tasks)
                                || ctx.stream_mode.contains(&StreamMode::Debug)
                            {
                                let _ = tx
                                    .send(StreamEvent::TaskEnd {
                                        node_id: current_id.clone(),
                                        result: Err(format!(
                                            "interrupted: {:?}",
                                            interrupt.0.value
                                        )),
                                    })
                                    .await;
                            }
                        }
                    }

                    // Log and return the interrupt error
                    log_graph_error(&AgentError::Interrupted(interrupt.clone()));
                    return Err(AgentError::Interrupted(interrupt.clone()));
                }
                Err(e) => {
                    // Emit TaskEnd event with error if Tasks or Debug mode is enabled
                    if let Some(ctx) = run_ctx {
                        if let Some(tx) = &ctx.stream_tx {
                            if ctx.stream_mode.contains(&StreamMode::Tasks)
                                || ctx.stream_mode.contains(&StreamMode::Debug)
                            {
                                let _ = tx
                                    .send(StreamEvent::TaskEnd {
                                        node_id: current_id.clone(),
                                        result: Err(e.to_string()),
                                    })
                                    .await;
                            }
                        }
                    }
                    log_graph_error(&e);
                    return Err(e);
                }
            };

            // Emit TaskEnd event with success if Tasks or Debug mode is enabled
            if let Some(ctx) = run_ctx {
                if let Some(tx) = &ctx.stream_tx {
                    if ctx.stream_mode.contains(&StreamMode::Tasks)
                        || ctx.stream_mode.contains(&StreamMode::Debug)
                    {
                        let _ = tx
                            .send(StreamEvent::TaskEnd {
                                node_id: current_id.clone(),
                                result: Ok(()),
                            })
                            .await;
                    }
                }
            }

            // Log node completion
            log_node_complete(current_id, &next);

            // Apply state update using the configured updater
            self.state_updater.apply_update(state, &new_state);

            // Log state update
            log_state_update(current_id);

            if let Some(ctx) = run_ctx {
                if let Some(tx) = &ctx.stream_tx {
                    if ctx.stream_mode.contains(&StreamMode::Values) {
                        let _ = tx.send(StreamEvent::Values(state.clone())).await;
                    }
                    if ctx.stream_mode.contains(&StreamMode::Updates) {
                        let _ = tx
                            .send(StreamEvent::Updates {
                                node_id: current_id.clone(),
                                state: state.clone(),
                            })
                            .await;
                    }
                }
            }

            let next_id: Option<String> =
                if let Some(NextEntry::Conditional(router)) = self.next_map.get(current_id) {
                    let target = router.resolve_next(state);
                    tracing::debug!(
                        from = %current_id,
                        to = %target,
                        "conditional routing"
                    );
                    Some(target)
                } else {
                    match next {
                        Next::End => None,
                        Next::Node(id) => Some(id),
                        Next::Continue => self
                            .next_map
                            .get(current_id)
                            .and_then(|e| {
                                if let NextEntry::Unconditional(id) = e {
                                    Some(id.clone())
                                } else {
                                    None
                                }
                            })
                            .or_else(|| {
                                let pos = self.edge_order.iter().position(|x| x == current_id)?;
                                self.edge_order.get(pos + 1).cloned()
                            }),
                    }
                };

            let should_end = next_id.is_none() || next_id.as_deref() == Some(END);
            if should_end {
                if let (Some(cp), Some(cfg)) = (&self.checkpointer, config) {
                    if cfg.thread_id.is_some() {
                        let checkpoint =
                            Checkpoint::from_state(state.clone(), CheckpointSource::Update, 0);
                        let _ = cp.put(cfg, &checkpoint).await;
                        if let Some(ctx) = run_ctx {
                            if let Some(tx) = &ctx.stream_tx {
                                if ctx.stream_mode.contains(&StreamMode::Checkpoints)
                                    || ctx.stream_mode.contains(&StreamMode::Debug)
                                {
                                    let checkpoint_ns = if cfg.checkpoint_ns.is_empty() {
                                        None
                                    } else {
                                        Some(cfg.checkpoint_ns.clone())
                                    };
                                    let _ = tx
                                        .send(StreamEvent::Checkpoint(
                                            crate::stream::CheckpointEvent {
                                                checkpoint_id: checkpoint.id.clone(),
                                                timestamp: checkpoint.ts.clone(),
                                                step: checkpoint.metadata.step,
                                                state: state.clone(),
                                                thread_id: cfg.thread_id.clone(),
                                                checkpoint_ns,
                                            },
                                        ))
                                        .await;
                                }
                            }
                        }
                    }
                }
                log_graph_complete();
                return Ok(());
            }
            if let Some(id) = next_id {
                *current_id = id;
            }
        }
    }

    /// Runs the graph with the given state. Starts at the first node in edge order;
    /// after each node, uses returned `Next` to continue linear order, jump to a node, or end.
    ///
    /// When `config` has `thread_id` and the graph was compiled with a checkpointer,
    /// the final state is saved after the run. Pass `None` for config to keep current behavior (no persistence).
    ///
    /// **Tool context**: A minimal [`RunContext`] is always built from `config` (or default)
    /// and passed to nodes. This ensures tools executed by ActNode receive full
    /// [`ToolCallContext`](crate::tool_source::ToolCallContext) (e.g. `thread_id`, `user_id`)
    /// when provided in config.
    ///
    /// - `Next::Continue`: run the next node in edge_order, or end if last.
    /// - `Next::Node(id)`: run the node with that id next.
    /// - `Next::End`: stop and return current state.
    pub async fn invoke(&self, state: S, config: Option<RunnableConfig>) -> Result<S, AgentError> {
        if self.nodes.is_empty() || !self.nodes.contains_key(&self.first_node_id) {
            return Err(AgentError::ExecutionFailed("empty graph".into()));
        }
        let config = config.unwrap_or_default();
        let run_ctx = RunContext::new(config.clone());
        let mut state = state;
        let mut current_id = run_ctx
            .config
            .resume_from_node_id
            .as_ref()
            .filter(|id| self.nodes.contains_key(id.as_str()))
            .cloned()
            .unwrap_or_else(|| self.first_node_id.clone());

        self.run_loop_inner(&mut state, &Some(config), &mut current_id, Some(&run_ctx))
            .await?;

        Ok(state)
    }

    /// Runs the graph with a fully configured RunContext.
    ///
    /// This method provides more control over the execution context, allowing you to:
    /// - Pass a custom store for long-term memory
    /// - Set the previous state (for resuming from checkpoints)
    /// - Provide custom runtime context data
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use graphweave::graph::RunContext;
    /// use graphweave::memory::{RunnableConfig, InMemoryStore};
    /// use std::sync::Arc;
    ///
    /// // Create a context with store and custom data
    /// let config = RunnableConfig::default();
    /// let store = Arc::new(InMemoryStore::new());
    /// let ctx = RunContext::<MyState>::new(config)
    ///     .with_store(store)
    ///     .with_runtime_context(serde_json::json!({"user_id": "123"}));
    ///
    /// // Invoke with context (async)
    /// let result = graph.invoke_with_context(initial_state, ctx).await?;
    /// ```
    pub async fn invoke_with_context(
        &self,
        state: S,
        run_ctx: RunContext<S>,
    ) -> Result<S, AgentError> {
        let mut state = state;
        let mut current_id = run_ctx
            .config
            .resume_from_node_id
            .as_ref()
            .filter(|id| self.nodes.contains_key(id.as_str()))
            .cloned()
            .unwrap_or_else(|| self.first_node_id.clone());

        let config = Some(run_ctx.config.clone());
        self.run_loop_inner(&mut state, &config, &mut current_id, Some(&run_ctx))
            .await?;

        Ok(state)
    }

    /// Streams graph execution, emitting events via channel-backed Stream.
    pub fn stream(
        &self,
        state: S,
        config: Option<RunnableConfig>,
        stream_mode: impl Into<HashSet<StreamMode>>,
    ) -> ReceiverStream<StreamEvent<S>> {
        let (tx, rx) = mpsc::channel(128);
        let graph = self.clone();
        let mode_set: HashSet<StreamMode> = stream_mode.into();

        tokio::spawn(async move {
            let mut state = state;
            let mut current_id = match graph.edge_order.first().cloned() {
                Some(id) => id,
                None => return,
            };
            let mut run_ctx = RunContext::new(config.clone().unwrap_or_default());
            run_ctx.stream_tx = Some(tx);
            run_ctx.stream_mode = mode_set;

            let _ = graph
                .run_loop_inner(&mut state, &config, &mut current_id, Some(&run_ctx))
                .await;
        });

        ReceiverStream::new(rx)
    }

    /// Returns the long-term store if the graph was compiled with `with_store(store)`.
    ///
    /// Nodes can use it for cross-thread memory (e.g. namespace from `config.user_id`).
    pub fn store(&self) -> Option<&Arc<dyn Store>> {
        self.store.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    use async_trait::async_trait;
    use tokio_stream::StreamExt;

    use crate::graph::{CompilationError, Next, Node, StateGraph, END, START};
    use crate::memory::{MemorySaver, RunnableConfig};
    use crate::stream::{StreamEvent, StreamMode};

    /// **Scenario**: When edge_order is empty, invoke returns ExecutionFailed("empty graph").
    #[tokio::test]
    async fn invoke_empty_graph_returns_execution_failed() {
        let graph = CompiledStateGraph::<crate::state::ReActState> {
            nodes: HashMap::new(),
            first_node_id: String::new(),
            edge_order: vec![],
            next_map: HashMap::new(),
            checkpointer: None,
            store: None,
            middleware: None,
            state_updater: Arc::new(crate::channels::ReplaceUpdater),
            retry_policy: RetryPolicy::None,
            interrupt_handler: None,
        };
        let state = crate::state::ReActState::default();
        let result = graph.invoke(state, None).await;
        match &result {
            Err(AgentError::ExecutionFailed(msg)) => {
                assert!(msg.contains("empty graph"), "{}", msg)
            }
            _ => panic!(
                "expected ExecutionFailed(\"empty graph\"), got {:?}",
                result
            ),
        }
    }

    #[derive(Clone)]
    struct AddNode {
        id: &'static str,
        delta: i32,
    }

    #[async_trait]
    impl Node<i32> for AddNode {
        fn id(&self) -> &str {
            self.id
        }

        async fn run(&self, state: i32) -> Result<(i32, Next), AgentError> {
            Ok((state + self.delta, Next::Continue))
        }
    }

    /// Node that returns Next::End after one step (covers run_loop Next::End + checkpoint path).
    #[derive(Clone)]
    struct EndAfterNode {
        id: &'static str,
        delta: i32,
    }

    #[async_trait]
    impl Node<i32> for EndAfterNode {
        fn id(&self) -> &str {
            self.id
        }
        async fn run(&self, state: i32) -> Result<(i32, Next), AgentError> {
            Ok((state + self.delta, Next::End))
        }
    }

    /// Node that from "first" returns Next::Node("third") to skip "second"; otherwise Continue.
    #[derive(Clone)]
    struct JumpToThirdNode {
        id: &'static str,
        delta: i32,
    }

    #[async_trait]
    impl Node<i32> for JumpToThirdNode {
        fn id(&self) -> &str {
            self.id
        }
        async fn run(&self, state: i32) -> Result<(i32, Next), AgentError> {
            let next = if self.id == "first" {
                Next::Node("third".to_string())
            } else {
                Next::Continue
            };
            Ok((state + self.delta, next))
        }
    }

    fn build_two_step_graph() -> CompiledStateGraph<i32> {
        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "first",
            Arc::new(AddNode {
                id: "first",
                delta: 1,
            }),
        );
        graph.add_node(
            "second",
            Arc::new(AddNode {
                id: "second",
                delta: 2,
            }),
        );
        graph.add_edge(START, "first");
        graph.add_edge("first", "second");
        graph.add_edge("second", END);
        graph.compile().expect("graph compiles")
    }

    /// Node that multiplies state by a constant (used inside a subgraph).
    #[derive(Clone)]
    struct MulNode {
        id: &'static str,
        factor: i32,
    }

    #[async_trait]
    impl Node<i32> for MulNode {
        fn id(&self) -> &str {
            self.id
        }
        async fn run(&self, state: i32) -> Result<(i32, Next), AgentError> {
            Ok((state * self.factor, Next::Continue))
        }
    }

    /// Wrapper node that runs a compiled subgraph with the current state.
    /// Connects the outer graph's next node to the inner graph (subgraph-as-node).
    #[derive(Clone)]
    struct SubgraphNode {
        id: &'static str,
        inner: CompiledStateGraph<i32>,
    }

    #[async_trait]
    impl Node<i32> for SubgraphNode {
        fn id(&self) -> &str {
            self.id
        }
        async fn run(&self, state: i32) -> Result<(i32, Next), AgentError> {
            let new_state = self.inner.invoke(state, None).await?;
            Ok((new_state, Next::Continue))
        }
    }

    fn build_subgraph_b() -> Result<CompiledStateGraph<i32>, CompilationError> {
        let mut graph = StateGraph::<i32>::new();
        graph
            .add_node("mul10", Arc::new(MulNode { id: "mul10", factor: 10 }))
            .add_edge(START, "mul10")
            .add_edge("mul10", END);
        graph.compile()
    }

    /// **Scenario**: Graph A runs node a1, then a2, then executes Graph B as a single node.
    /// Graph A's next node (after a2) is connected to Graph B via SubgraphNode.
    /// State flow: 0 → a1(+1)=1 → a2(+2)=3 → graph_b(*10)=30.
    #[tokio::test]
    async fn invoke_graph_a_then_subgraph_b_as_node_produces_expected_state() {
        let compiled_b = build_subgraph_b().expect("graph B compiles");

        let mut graph_a = StateGraph::<i32>::new();
        graph_a
            .add_node("a1", Arc::new(AddNode { id: "a1", delta: 1 }))
            .add_node("a2", Arc::new(AddNode { id: "a2", delta: 2 }))
            .add_node(
                "subgraph_b",
                Arc::new(SubgraphNode {
                    id: "subgraph_b",
                    inner: compiled_b,
                }),
            )
            .add_edge(START, "a1")
            .add_edge("a1", "a2")
            .add_edge("a2", "subgraph_b")
            .add_edge("subgraph_b", END);

        let compiled_a = graph_a.compile().expect("graph A compiles");

        let initial: i32 = 0;
        let final_state = compiled_a.invoke(initial, None).await.unwrap();

        assert_eq!(final_state, 30, "0 -> a1(1) -> a2(3) -> graph_b(30)");
    }

    /// **Scenario**: Graph with conditional edges routes to the correct node based on state.
    #[tokio::test]
    async fn invoke_conditional_edges_routes_by_state() {
        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "decide",
            Arc::new(AddNode {
                id: "decide",
                delta: 0,
            }),
        );
        graph.add_node(
            "even_node",
            Arc::new(AddNode {
                id: "even_node",
                delta: 10,
            }),
        );
        graph.add_node(
            "odd_node",
            Arc::new(AddNode {
                id: "odd_node",
                delta: 100,
            }),
        );
        graph.add_edge(START, "decide");
        graph.add_edge("even_node", END);
        graph.add_edge("odd_node", END);
        let path_map: HashMap<String, String> = [
            ("even".to_string(), "even_node".to_string()),
            ("odd".to_string(), "odd_node".to_string()),
        ]
        .into_iter()
        .collect();
        graph.add_conditional_edges(
            "decide",
            Arc::new(|s: &i32| {
                if s % 2 == 0 {
                    "even".into()
                } else {
                    "odd".into()
                }
            }),
            Some(path_map),
        );
        let compiled = graph.compile().expect("graph compiles");
        let out_even = compiled.invoke(2, None).await.unwrap();
        assert_eq!(out_even, 12, "state 2 -> even_node -> +10");
        let out_odd = compiled.invoke(1, None).await.unwrap();
        assert_eq!(out_odd, 101, "state 1 -> odd_node -> +100");
    }

    /// **Scenario**: Conditional edges with no path_map use router return value as node id.
    #[tokio::test]
    async fn invoke_conditional_edges_no_path_map_uses_key_as_node_id() {
        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "decide",
            Arc::new(AddNode {
                id: "decide",
                delta: 0,
            }),
        );
        graph.add_node(
            "go_a",
            Arc::new(AddNode {
                id: "go_a",
                delta: 1,
            }),
        );
        graph.add_node(
            "go_b",
            Arc::new(AddNode {
                id: "go_b",
                delta: 10,
            }),
        );
        graph.add_edge(START, "decide");
        graph.add_edge("go_a", END);
        graph.add_edge("go_b", END);
        graph.add_conditional_edges(
            "decide",
            Arc::new(|s: &i32| if *s > 0 { "go_a".into() } else { "go_b".into() }),
            None,
        );
        let compiled = graph.compile().expect("graph compiles");
        assert_eq!(
            compiled.invoke(1, None).await.unwrap(),
            2,
            "s>0 -> go_a -> +1"
        );
        assert_eq!(
            compiled.invoke(0, None).await.unwrap(),
            10,
            "s<=0 -> go_b -> +10"
        );
    }

    /// **Scenario**: invoke with checkpointer and config.thread_id saves checkpoint at end of run.
    #[tokio::test]
    async fn invoke_with_checkpointer_and_thread_id_saves_checkpoint() {
        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "first",
            Arc::new(AddNode {
                id: "first",
                delta: 1,
            }),
        );
        graph.add_node(
            "second",
            Arc::new(AddNode {
                id: "second",
                delta: 2,
            }),
        );
        graph.add_edge(START, "first");
        graph.add_edge("first", "second");
        graph.add_edge("second", END);
        let cp = Arc::new(MemorySaver::<i32>::new());
        let compiled = graph
            .compile_with_checkpointer(cp.clone())
            .expect("graph compiles");
        let config = RunnableConfig {
            thread_id: Some("tid-cp".into()),
            checkpoint_id: None,
            checkpoint_ns: String::new(),
            user_id: None,
            resume_from_node_id: None,
        };
        let out = compiled.invoke(0, Some(config)).await.unwrap();
        assert_eq!(out, 3);
        let cfg = RunnableConfig {
            thread_id: Some("tid-cp".into()),
            ..Default::default()
        };
        let tuple = cp.get_tuple(&cfg).await.unwrap();
        assert!(tuple.is_some(), "checkpoint should be saved");
    }

    /// **Scenario**: Node returning Next::End triggers checkpoint save when checkpointer and thread_id set.
    #[tokio::test]
    async fn invoke_next_end_with_checkpointer_saves_checkpoint() {
        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "only",
            Arc::new(EndAfterNode {
                id: "only",
                delta: 5,
            }),
        );
        graph.add_edge(START, "only");
        graph.add_edge("only", END);
        let cp = Arc::new(MemorySaver::<i32>::new());
        let compiled = graph
            .compile_with_checkpointer(cp.clone())
            .expect("graph compiles");
        let config = RunnableConfig {
            thread_id: Some("tid-end".into()),
            ..Default::default()
        };
        let out = compiled.invoke(0, Some(config)).await.unwrap();
        assert_eq!(out, 5);
        let tuple = cp
            .get_tuple(&RunnableConfig {
                thread_id: Some("tid-end".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(tuple.is_some(), "checkpoint on Next::End should be saved");
    }

    /// **Scenario**: Node returning Next::Node(id) jumps to that node (covers run_loop Next::Node branch).
    #[tokio::test]
    async fn invoke_next_node_jumps_to_specified_node() {
        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "first",
            Arc::new(JumpToThirdNode {
                id: "first",
                delta: 1,
            }),
        );
        graph.add_node(
            "second",
            Arc::new(AddNode {
                id: "second",
                delta: 10,
            }),
        );
        graph.add_node(
            "third",
            Arc::new(AddNode {
                id: "third",
                delta: 100,
            }),
        );
        graph.add_edge(START, "first");
        graph.add_edge("first", "second");
        graph.add_edge("second", "third");
        graph.add_edge("third", END);
        let compiled = graph.compile().expect("graph compiles");
        let out = compiled.invoke(0, None).await.unwrap();
        // first: 0+1=1, returns Next::Node("third"); then third: 1+100=101 (second skipped).
        assert_eq!(out, 101);
    }

    /// **Scenario**: stream(values) emits state snapshots per node and ends with final state.
    #[tokio::test]
    async fn stream_values_emits_states() {
        let graph = build_two_step_graph();
        let stream = graph.stream(0, None, HashSet::from_iter([StreamMode::Values]));
        let events: Vec<_> = stream.collect().await;
        assert!(!events.is_empty(), "expected at least one Values event");
        assert!(
            matches!(events.last(), Some(StreamEvent::Values(v)) if *v == 3),
            "last event should be final state 3"
        );
    }

    /// **Scenario**: stream(updates) emits Updates with node ids in order.
    #[tokio::test]
    async fn stream_updates_emit_node_ids_in_order() {
        let graph = build_two_step_graph();
        let stream = graph.stream(0, None, HashSet::from_iter([StreamMode::Updates]));
        let events: Vec<_> = stream.collect().await;
        let ids: Vec<_> = events
            .iter()
            .map(|e| match e {
                StreamEvent::Updates { node_id, state } => {
                    assert!(
                        *state == 1 || *state == 3,
                        "unexpected state value {}",
                        state
                    );
                    node_id.clone()
                }
                other => panic!("unexpected event {:?}", other),
            })
            .collect();
        assert_eq!(ids, vec!["first".to_string(), "second".to_string()]);
    }

    /// **Scenario**: Empty graph stream() does not panic and yields zero events.
    #[tokio::test]
    async fn stream_empty_graph_no_panic_zero_events() {
        let graph = CompiledStateGraph::<i32> {
            nodes: HashMap::new(),
            first_node_id: String::new(),
            edge_order: vec![],
            next_map: HashMap::new(),
            checkpointer: None,
            store: None,
            middleware: None,
            state_updater: Arc::new(crate::channels::ReplaceUpdater),
            retry_policy: RetryPolicy::None,
            interrupt_handler: None,
        };
        let stream = graph.stream(0, None, HashSet::from_iter([StreamMode::Values]));
        let events: Vec<_> = stream.collect().await;
        assert!(
            events.is_empty(),
            "empty graph should emit 0 events, got {}",
            events.len()
        );
    }

    fn build_single_node_graph() -> CompiledStateGraph<i32> {
        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "only",
            Arc::new(AddNode {
                id: "only",
                delta: 10,
            }),
        );
        graph.add_edge(START, "only");
        graph.add_edge("only", END);
        graph.compile().expect("graph compiles")
    }

    /// **Scenario**: Single-node graph stream(Values+Updates) emits exactly one Values and one Updates.
    #[tokio::test]
    async fn stream_single_node_emits_one_values_one_updates() {
        let graph = build_single_node_graph();
        let stream = graph.stream(
            0,
            None,
            HashSet::from_iter([StreamMode::Values, StreamMode::Updates]),
        );
        let events: Vec<_> = stream.collect().await;
        assert_eq!(events.len(), 2, "single node: one Values + one Updates");
        match &events[0] {
            StreamEvent::Values(s) => assert_eq!(*s, 10),
            other => panic!("first event should be Values(10), got {:?}", other),
        }
        match &events[1] {
            StreamEvent::Updates { node_id, state } => {
                assert_eq!(node_id, "only");
                assert_eq!(*state, 10);
            }
            other => panic!("second event should be Updates(only, 10), got {:?}", other),
        }
    }

    /// **Scenario**: stream(Values+Updates) emits both variants; per node order is Values then Updates.
    #[tokio::test]
    async fn stream_values_and_updates_both_enabled() {
        let graph = build_two_step_graph();
        let stream = graph.stream(
            0,
            None,
            HashSet::from_iter([StreamMode::Values, StreamMode::Updates]),
        );
        let events: Vec<_> = stream.collect().await;
        assert_eq!(events.len(), 4, "two nodes: two Values + two Updates");
        match &events[0] {
            StreamEvent::Values(s) => assert_eq!(*s, 1),
            _ => panic!("events[0] should be Values(1)"),
        }
        match &events[1] {
            StreamEvent::Updates { node_id, .. } => assert_eq!(node_id, "first"),
            _ => panic!("events[1] should be Updates(first, ...)"),
        }
        match &events[2] {
            StreamEvent::Values(s) => assert_eq!(*s, 3),
            _ => panic!("events[2] should be Values(3)"),
        }
        match &events[3] {
            StreamEvent::Updates { node_id, .. } => assert_eq!(node_id, "second"),
            _ => panic!("events[3] should be Updates(second, ...)"),
        }
    }

    /// **Scenario**: stream with Some(config) completes without panic and yields same events as None.
    #[tokio::test]
    async fn stream_with_some_config_no_panic() {
        let graph = build_two_step_graph();
        let config = RunnableConfig {
            thread_id: Some("tid".into()),
            checkpoint_id: None,
            checkpoint_ns: String::new(),
            user_id: Some("u1".into()),
            resume_from_node_id: None,
        };
        let stream = graph.stream(0, Some(config), HashSet::from_iter([StreamMode::Values]));
        let events: Vec<_> = stream.collect().await;
        assert!(!events.is_empty());
        assert!(matches!(events.last(), Some(StreamEvent::Values(v)) if *v == 3));
    }

    /// **Scenario**: stream_mode containing Messages and Custom still collects without panic; run_loop only sends Values/Updates.
    #[tokio::test]
    async fn stream_mode_includes_messages_custom_collect_no_panic() {
        let graph = build_two_step_graph();
        let stream = graph.stream(
            0,
            None,
            HashSet::from_iter([
                StreamMode::Values,
                StreamMode::Updates,
                StreamMode::Messages,
                StreamMode::Custom,
            ]),
        );
        let events: Vec<_> = stream.collect().await;
        assert!(!events.is_empty());
        for e in &events {
            match e {
                StreamEvent::Values(_) | StreamEvent::Updates { .. } => {}
                StreamEvent::TotExpand { .. }
                | StreamEvent::TotEvaluate { .. }
                | StreamEvent::TotBacktrack { .. } => {}
                StreamEvent::GotPlan { .. }
                | StreamEvent::GotNodeStart { .. }
                | StreamEvent::GotNodeComplete { .. }
                | StreamEvent::GotNodeFailed { .. }
                | StreamEvent::GotExpand { .. } => {}
                StreamEvent::Messages { .. }
                | StreamEvent::Custom(_)
                | StreamEvent::Checkpoint(_)
                | StreamEvent::TaskStart { .. }
                | StreamEvent::TaskEnd { .. }
                | StreamEvent::Usage { .. } => {
                    panic!(
                        "run_loop does not emit Messages/Custom/Checkpoint/Task/Usage events in this test, got {:?}",
                        e
                    )
                }
            }
        }
        assert_eq!(events.len(), 4, "only Values and Updates from run_loop");
    }

    // === Runtime Integration Tests ===

    /// **Scenario**: invoke_with_context executes graph with RunContext and produces correct result.
    #[tokio::test]
    async fn invoke_with_context_basic() {
        let graph = build_two_step_graph();
        let config = RunnableConfig::default();
        let ctx = crate::graph::RunContext::<i32>::new(config);
        let result = graph.invoke_with_context(0, ctx).await.unwrap();
        assert_eq!(
            result, 3,
            "invoke_with_context should produce same result as invoke"
        );
    }

    /// **Scenario**: invoke_with_context with store passes store to RunContext.
    #[tokio::test]
    async fn invoke_with_context_with_store() {
        use crate::memory::InMemoryStore;

        let graph = build_two_step_graph();
        let config = RunnableConfig::default();
        let store = Arc::new(InMemoryStore::new());
        let ctx = crate::graph::RunContext::<i32>::new(config).with_store(store.clone());

        assert!(ctx.store().is_some(), "store should be set");
        let result = graph.invoke_with_context(0, ctx).await.unwrap();
        assert_eq!(result, 3);
    }

    /// **Scenario**: invoke_with_context with runtime_context passes custom context data.
    #[tokio::test]
    async fn invoke_with_context_with_runtime_context() {
        let graph = build_two_step_graph();
        let config = RunnableConfig::default();
        let ctx = crate::graph::RunContext::<i32>::new(config)
            .with_runtime_context(serde_json::json!({"user_id": "test_user", "session": 123}));

        assert!(
            ctx.runtime_context().is_some(),
            "runtime_context should be set"
        );
        let runtime_ctx = ctx.runtime_context().unwrap();
        assert_eq!(runtime_ctx["user_id"], "test_user");
        assert_eq!(runtime_ctx["session"], 123);

        let result = graph.invoke_with_context(0, ctx).await.unwrap();
        assert_eq!(result, 3);
    }

    /// **Scenario**: invoke_with_context with previous state sets previous correctly.
    #[tokio::test]
    async fn invoke_with_context_with_previous() {
        let graph = build_two_step_graph();
        let config = RunnableConfig::default();
        let ctx = crate::graph::RunContext::<i32>::new(config).with_previous(100);

        assert_eq!(ctx.previous(), Some(&100), "previous should be set to 100");
        let result = graph.invoke_with_context(0, ctx).await.unwrap();
        assert_eq!(result, 3);
    }

    // === StateUpdater Integration Tests ===

    #[derive(Clone, Debug, PartialEq)]
    struct MessageState {
        messages: Vec<String>,
        count: i32,
    }

    /// Node that adds a message and increments count
    #[derive(Clone)]
    struct AddMessageNode {
        id: &'static str,
        message: &'static str,
    }

    #[async_trait]
    impl Node<MessageState> for AddMessageNode {
        fn id(&self) -> &str {
            self.id
        }

        async fn run(&self, _state: MessageState) -> Result<(MessageState, Next), AgentError> {
            // Return just the new message and count increment
            Ok((
                MessageState {
                    messages: vec![self.message.to_string()],
                    count: 1,
                },
                Next::Continue,
            ))
        }
    }

    /// **Scenario**: Custom StateUpdater appends messages instead of replacing.
    #[tokio::test]
    async fn invoke_with_custom_state_updater_appends_messages() {
        use crate::channels::FieldBasedUpdater;

        // Create graph with custom updater that appends messages
        let updater =
            FieldBasedUpdater::new(|current: &mut MessageState, update: &MessageState| {
                current.messages.extend(update.messages.iter().cloned());
                current.count += update.count;
            });

        let mut graph = StateGraph::<MessageState>::new().with_state_updater(Arc::new(updater));

        graph.add_node(
            "first",
            Arc::new(AddMessageNode {
                id: "first",
                message: "Hello",
            }),
        );
        graph.add_node(
            "second",
            Arc::new(AddMessageNode {
                id: "second",
                message: "World",
            }),
        );
        graph.add_edge(START, "first");
        graph.add_edge("first", "second");
        graph.add_edge("second", END);

        let compiled = graph.compile().expect("graph compiles");

        let initial_state = MessageState {
            messages: vec!["Start".to_string()],
            count: 0,
        };

        let result = compiled.invoke(initial_state, None).await.unwrap();

        // With custom updater, messages should be appended
        assert_eq!(
            result.messages,
            vec![
                "Start".to_string(),
                "Hello".to_string(),
                "World".to_string()
            ],
            "messages should be appended"
        );
        assert_eq!(result.count, 2, "count should be incremented twice");
    }

    /// **Scenario**: Default behavior (ReplaceUpdater) replaces entire state.
    #[tokio::test]
    async fn invoke_default_replaces_state() {
        let mut graph = StateGraph::<MessageState>::new();

        graph.add_node(
            "first",
            Arc::new(AddMessageNode {
                id: "first",
                message: "Hello",
            }),
        );
        graph.add_node(
            "second",
            Arc::new(AddMessageNode {
                id: "second",
                message: "World",
            }),
        );
        graph.add_edge(START, "first");
        graph.add_edge("first", "second");
        graph.add_edge("second", END);

        let compiled = graph.compile().expect("graph compiles");

        let initial_state = MessageState {
            messages: vec!["Start".to_string()],
            count: 0,
        };

        let result = compiled.invoke(initial_state, None).await.unwrap();

        // With default ReplaceUpdater, only the last state should remain
        assert_eq!(
            result.messages,
            vec!["World".to_string()],
            "messages should be replaced"
        );
        assert_eq!(result.count, 1, "count should be 1 from last node");
    }

    // === Retry Mechanism Tests ===

    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Node that fails a specified number of times before succeeding.
    #[derive(Clone)]
    struct FailingNode {
        id: &'static str,
        fail_count: Arc<AtomicUsize>,
        max_failures: usize,
    }

    #[async_trait]
    impl Node<i32> for FailingNode {
        fn id(&self) -> &str {
            self.id
        }

        async fn run(&self, state: i32) -> Result<(i32, Next), AgentError> {
            let current = self.fail_count.fetch_add(1, Ordering::SeqCst);
            if current < self.max_failures {
                Err(AgentError::ExecutionFailed(format!(
                    "Deliberate failure {} of {}",
                    current + 1,
                    self.max_failures
                )))
            } else {
                Ok((state + 10, Next::Continue))
            }
        }
    }

    /// **Scenario**: Node with retry policy succeeds after transient failures.
    #[tokio::test]
    async fn invoke_with_retry_succeeds_after_failures() {
        let fail_count = Arc::new(AtomicUsize::new(0));

        let mut graph = StateGraph::<i32>::new()
            .with_retry_policy(RetryPolicy::fixed(3, std::time::Duration::from_millis(10)));

        graph.add_node(
            "failing",
            Arc::new(FailingNode {
                id: "failing",
                fail_count: fail_count.clone(),
                max_failures: 2, // Fails twice, then succeeds
            }),
        );
        graph.add_edge(START, "failing");
        graph.add_edge("failing", END);

        let compiled = graph.compile().expect("graph compiles");
        let result = compiled.invoke(0, None).await.unwrap();

        // Node should have been called 3 times (2 failures + 1 success)
        assert_eq!(fail_count.load(Ordering::SeqCst), 3);
        assert_eq!(result, 10);
    }

    /// **Scenario**: Node without retry fails immediately.
    #[tokio::test]
    async fn invoke_without_retry_fails_immediately() {
        let fail_count = Arc::new(AtomicUsize::new(0));

        let mut graph = StateGraph::<i32>::new(); // No retry policy (default: None)

        graph.add_node(
            "failing",
            Arc::new(FailingNode {
                id: "failing",
                fail_count: fail_count.clone(),
                max_failures: 2,
            }),
        );
        graph.add_edge(START, "failing");
        graph.add_edge("failing", END);

        let compiled = graph.compile().expect("graph compiles");
        let result = compiled.invoke(0, None).await;

        // Node should have been called only once
        assert_eq!(fail_count.load(Ordering::SeqCst), 1);
        assert!(result.is_err());
    }

    /// **Scenario**: Node exhausts retry attempts and fails.
    #[tokio::test]
    async fn invoke_with_retry_exhausted_fails() {
        let fail_count = Arc::new(AtomicUsize::new(0));

        let mut graph = StateGraph::<i32>::new()
            .with_retry_policy(RetryPolicy::fixed(2, std::time::Duration::from_millis(10)));

        graph.add_node(
            "failing",
            Arc::new(FailingNode {
                id: "failing",
                fail_count: fail_count.clone(),
                max_failures: 5, // Fails more than retry limit
            }),
        );
        graph.add_edge(START, "failing");
        graph.add_edge("failing", END);

        let compiled = graph.compile().expect("graph compiles");
        let result = compiled.invoke(0, None).await;

        // Node should have been called 3 times (initial + 2 retries)
        assert_eq!(fail_count.load(Ordering::SeqCst), 3);
        assert!(result.is_err());
    }

    // === Checkpoints Streaming Tests ===

    /// **Scenario**: stream() emits checkpoint events when Checkpoints mode is enabled and checkpointer is present.
    #[tokio::test]
    async fn stream_checkpoints_emits_checkpoint_events_with_checkpointer() {
        use crate::memory::MemorySaver;

        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "add_one",
            Arc::new(AddNode {
                id: "add_one",
                delta: 1,
            }),
        );
        graph.add_node(
            "add_two",
            Arc::new(AddNode {
                id: "add_two",
                delta: 2,
            }),
        );
        graph.add_edge(START, "add_one");
        graph.add_edge("add_one", "add_two");
        graph.add_edge("add_two", END);

        let checkpointer = Arc::new(MemorySaver::new());
        let compiled = graph
            .compile_with_checkpointer(checkpointer)
            .expect("graph compiles");

        // Use thread_id to enable checkpoint saving
        let config = RunnableConfig {
            thread_id: Some("test-thread".into()),
            ..Default::default()
        };

        let stream = compiled.stream(
            0,
            Some(config.clone()),
            HashSet::from_iter([
                StreamMode::Values,
                StreamMode::Updates,
                StreamMode::Checkpoints,
            ]),
        );
        let events: Vec<_> = stream.collect().await;

        // Should have Values, Updates, and at least one Checkpoint event
        let checkpoint_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, StreamEvent::Checkpoint(_)))
            .collect();

        assert!(
            !checkpoint_events.is_empty(),
            "should have at least one checkpoint event, got {} total events",
            events.len()
        );

        // Verify checkpoint event content
        if let StreamEvent::Checkpoint(cp) = checkpoint_events.last().unwrap() {
            assert!(
                !cp.checkpoint_id.is_empty(),
                "checkpoint_id should not be empty"
            );
            assert!(!cp.timestamp.is_empty(), "timestamp should not be empty");
            assert_eq!(cp.thread_id, Some("test-thread".into()));
            assert_eq!(cp.state, 3, "final state should be 3 (0 + 1 + 2)");
        } else {
            panic!("expected Checkpoint event");
        }
    }

    /// **Scenario**: stream() does not emit checkpoint events when Checkpoints mode is disabled.
    #[tokio::test]
    async fn stream_no_checkpoint_events_without_checkpoints_mode() {
        use crate::memory::MemorySaver;

        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "add_one",
            Arc::new(AddNode {
                id: "add_one",
                delta: 1,
            }),
        );
        graph.add_edge(START, "add_one");
        graph.add_edge("add_one", END);

        let checkpointer = Arc::new(MemorySaver::new());
        let compiled = graph
            .compile_with_checkpointer(checkpointer)
            .expect("graph compiles");

        let config = RunnableConfig {
            thread_id: Some("test-thread".into()),
            ..Default::default()
        };

        // Stream without Checkpoints mode
        let stream = compiled.stream(
            0,
            Some(config),
            HashSet::from_iter([StreamMode::Values, StreamMode::Updates]),
        );
        let events: Vec<_> = stream.collect().await;

        // Should NOT have any Checkpoint events
        let checkpoint_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, StreamEvent::Checkpoint(_)))
            .collect();

        assert!(
            checkpoint_events.is_empty(),
            "should not have checkpoint events when Checkpoints mode is disabled"
        );
    }

    /// **Scenario**: stream() does not emit checkpoint events without checkpointer.
    #[tokio::test]
    async fn stream_no_checkpoint_events_without_checkpointer() {
        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "add_one",
            Arc::new(AddNode {
                id: "add_one",
                delta: 1,
            }),
        );
        graph.add_edge(START, "add_one");
        graph.add_edge("add_one", END);

        // Compile without checkpointer
        let compiled = graph.compile().expect("graph compiles");

        let config = RunnableConfig {
            thread_id: Some("test-thread".into()),
            ..Default::default()
        };

        // Stream with Checkpoints mode but no checkpointer
        let stream = compiled.stream(
            0,
            Some(config),
            HashSet::from_iter([
                StreamMode::Values,
                StreamMode::Updates,
                StreamMode::Checkpoints,
            ]),
        );
        let events: Vec<_> = stream.collect().await;

        // Should NOT have any Checkpoint events (no checkpointer)
        let checkpoint_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, StreamEvent::Checkpoint(_)))
            .collect();

        assert!(
            checkpoint_events.is_empty(),
            "should not have checkpoint events without checkpointer"
        );
    }

    // === Tasks Streaming Tests ===

    /// **Scenario**: stream() emits TaskStart and TaskEnd events when Tasks mode is enabled.
    #[tokio::test]
    async fn stream_tasks_emits_task_events() {
        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "add_one",
            Arc::new(AddNode {
                id: "add_one",
                delta: 1,
            }),
        );
        graph.add_node(
            "add_two",
            Arc::new(AddNode {
                id: "add_two",
                delta: 2,
            }),
        );
        graph.add_edge(START, "add_one");
        graph.add_edge("add_one", "add_two");
        graph.add_edge("add_two", END);

        let compiled = graph.compile().expect("graph compiles");

        let stream = compiled.stream(
            0,
            None,
            HashSet::from_iter([StreamMode::Values, StreamMode::Tasks]),
        );
        let events: Vec<_> = stream.collect().await;

        // Should have TaskStart and TaskEnd for each node (2 nodes = 4 task events)
        let task_start_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, StreamEvent::TaskStart { .. }))
            .collect();
        let task_end_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, StreamEvent::TaskEnd { .. }))
            .collect();

        assert_eq!(
            task_start_events.len(),
            2,
            "should have 2 TaskStart events (one per node)"
        );
        assert_eq!(
            task_end_events.len(),
            2,
            "should have 2 TaskEnd events (one per node)"
        );

        // Verify order: TaskStart -> TaskEnd for each node
        // First node: add_one
        if let StreamEvent::TaskStart { node_id } = task_start_events[0] {
            assert_eq!(node_id, "add_one");
        }
        if let StreamEvent::TaskEnd { node_id, result } = task_end_events[0] {
            assert_eq!(node_id, "add_one");
            assert!(result.is_ok());
        }
        // Second node: add_two
        if let StreamEvent::TaskStart { node_id } = task_start_events[1] {
            assert_eq!(node_id, "add_two");
        }
        if let StreamEvent::TaskEnd { node_id, result } = task_end_events[1] {
            assert_eq!(node_id, "add_two");
            assert!(result.is_ok());
        }
    }

    /// **Scenario**: stream() does not emit task events when Tasks mode is disabled.
    #[tokio::test]
    async fn stream_no_task_events_without_tasks_mode() {
        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "add_one",
            Arc::new(AddNode {
                id: "add_one",
                delta: 1,
            }),
        );
        graph.add_edge(START, "add_one");
        graph.add_edge("add_one", END);

        let compiled = graph.compile().expect("graph compiles");

        // Stream without Tasks mode
        let stream = compiled.stream(0, None, HashSet::from_iter([StreamMode::Values]));
        let events: Vec<_> = stream.collect().await;

        // Should NOT have any TaskStart or TaskEnd events
        let task_events: Vec<_> = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    StreamEvent::TaskStart { .. } | StreamEvent::TaskEnd { .. }
                )
            })
            .collect();

        assert!(
            task_events.is_empty(),
            "should not have task events when Tasks mode is disabled"
        );
    }

    /// **Scenario**: Debug mode emits both checkpoints and task events.
    #[tokio::test]
    async fn stream_debug_mode_emits_checkpoints_and_tasks() {
        use crate::memory::MemorySaver;

        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "add_one",
            Arc::new(AddNode {
                id: "add_one",
                delta: 1,
            }),
        );
        graph.add_edge(START, "add_one");
        graph.add_edge("add_one", END);

        let checkpointer = Arc::new(MemorySaver::new());
        let compiled = graph
            .compile_with_checkpointer(checkpointer)
            .expect("graph compiles");

        let config = RunnableConfig {
            thread_id: Some("test-thread".into()),
            ..Default::default()
        };

        // Stream with Debug mode only (should emit both checkpoints and tasks)
        let stream = compiled.stream(0, Some(config), HashSet::from_iter([StreamMode::Debug]));
        let events: Vec<_> = stream.collect().await;

        // Should have Checkpoint events (debug includes checkpoints)
        let checkpoint_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, StreamEvent::Checkpoint(_)))
            .collect();

        // Should have TaskStart and TaskEnd events (debug includes tasks)
        let task_events: Vec<_> = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    StreamEvent::TaskStart { .. } | StreamEvent::TaskEnd { .. }
                )
            })
            .collect();

        assert!(
            !checkpoint_events.is_empty(),
            "Debug mode should emit checkpoint events"
        );
        assert!(
            !task_events.is_empty(),
            "Debug mode should emit task events"
        );
    }

    // === Interrupt Handler Integration Tests ===

    /// A node that raises an interrupt after processing.
    struct InterruptingNode {
        id: &'static str,
        interrupt_value: serde_json::Value,
    }

    #[async_trait::async_trait]
    impl Node<i32> for InterruptingNode {
        fn id(&self) -> &str {
            self.id
        }

        async fn run(&self, _state: i32) -> Result<(i32, Next), AgentError> {
            use crate::graph::{GraphInterrupt, Interrupt};
            Err(AgentError::Interrupted(GraphInterrupt(Interrupt::new(
                self.interrupt_value.clone(),
            ))))
        }
    }

    /// **Scenario**: Node that raises an interrupt returns Interrupted error.
    #[tokio::test]
    async fn invoke_with_interrupting_node_returns_interrupted_error() {
        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "interrupt",
            Arc::new(InterruptingNode {
                id: "interrupt",
                interrupt_value: serde_json::json!({"action": "approve"}),
            }),
        );
        graph.add_edge(START, "interrupt");
        graph.add_edge("interrupt", END);

        let compiled = graph.compile().expect("graph compiles");
        let result = compiled.invoke(0, None).await;

        // Should return Interrupted error
        assert!(
            matches!(result, Err(AgentError::Interrupted(_))),
            "Expected Interrupted error, got {:?}",
            result
        );
    }

    /// **Scenario**: Interrupt with checkpointer saves checkpoint before returning error.
    #[tokio::test]
    async fn invoke_with_interrupt_saves_checkpoint_when_checkpointer_present() {
        use crate::memory::MemorySaver;

        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "add_one",
            Arc::new(AddNode {
                id: "add_one",
                delta: 1,
            }),
        );
        graph.add_node(
            "interrupt",
            Arc::new(InterruptingNode {
                id: "interrupt",
                interrupt_value: serde_json::json!({"action": "approve"}),
            }),
        );
        graph.add_edge(START, "add_one");
        graph.add_edge("add_one", "interrupt");
        graph.add_edge("interrupt", END);

        let checkpointer = Arc::new(MemorySaver::<i32>::new());
        let compiled = graph
            .compile_with_checkpointer(checkpointer.clone())
            .expect("graph compiles");

        let config = RunnableConfig {
            thread_id: Some("test-interrupt".to_string()),
            ..Default::default()
        };

        let result = compiled.invoke(0, Some(config.clone())).await;

        // Should return Interrupted error
        assert!(matches!(result, Err(AgentError::Interrupted(_))));

        // Checkpoint should have been saved with state = 1 (after add_one)
        use crate::memory::Checkpointer;
        let checkpoint = checkpointer.get_tuple(&config).await.unwrap();
        assert!(checkpoint.is_some(), "Checkpoint should have been saved");
        let (cp, _metadata) = checkpoint.unwrap();
        assert_eq!(cp.channel_values, 1, "State should be 1 after add_one node");
    }

    /// **Scenario**: Stream with interrupting node emits TaskEnd with error.
    #[tokio::test]
    async fn stream_with_interrupt_emits_task_end_with_error() {
        let mut graph = StateGraph::<i32>::new();
        graph.add_node(
            "interrupt",
            Arc::new(InterruptingNode {
                id: "interrupt",
                interrupt_value: serde_json::json!({"action": "approve"}),
            }),
        );
        graph.add_edge(START, "interrupt");
        graph.add_edge("interrupt", END);

        let compiled = graph.compile().expect("graph compiles");
        let stream = compiled.stream(0, None, HashSet::from_iter([StreamMode::Tasks]));
        let events: Vec<_> = stream.collect().await;

        // Should have TaskStart and TaskEnd events
        let task_end_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, StreamEvent::TaskEnd { .. }))
            .collect();

        assert_eq!(task_end_events.len(), 1, "Should have 1 TaskEnd event");

        if let StreamEvent::TaskEnd { node_id, result } = &task_end_events[0] {
            assert_eq!(node_id, "interrupt");
            assert!(result.is_err(), "TaskEnd should have error result");
            assert!(
                result.as_ref().unwrap_err().contains("interrupted"),
                "Error should mention interrupted"
            );
        } else {
            panic!("Expected TaskEnd event");
        }
    }

    /// A custom interrupt handler that records handled interrupts.
    struct RecordingInterruptHandler {
        handled: std::sync::Mutex<Vec<serde_json::Value>>,
    }

    impl RecordingInterruptHandler {
        fn new() -> Self {
            Self {
                handled: std::sync::Mutex::new(vec![]),
            }
        }

        fn handled_values(&self) -> Vec<serde_json::Value> {
            self.handled.lock().unwrap().clone()
        }
    }

    impl crate::graph::InterruptHandler for RecordingInterruptHandler {
        fn handle_interrupt(
            &self,
            interrupt: &crate::graph::Interrupt,
        ) -> Result<serde_json::Value, AgentError> {
            self.handled.lock().unwrap().push(interrupt.value.clone());
            Ok(serde_json::json!({"handled": true}))
        }
    }

    /// **Scenario**: Interrupt handler is called when interrupt occurs.
    #[tokio::test]
    async fn invoke_with_interrupt_handler_calls_handler() {
        let handler = Arc::new(RecordingInterruptHandler::new());

        let mut graph = StateGraph::<i32>::new().with_interrupt_handler(handler.clone());
        graph.add_node(
            "interrupt",
            Arc::new(InterruptingNode {
                id: "interrupt",
                interrupt_value: serde_json::json!({"action": "approve", "item": "order_123"}),
            }),
        );
        graph.add_edge(START, "interrupt");
        graph.add_edge("interrupt", END);

        let compiled = graph.compile().expect("graph compiles");
        let _ = compiled.invoke(0, None).await;

        // Handler should have been called with the interrupt value
        let handled = handler.handled_values();
        assert_eq!(handled.len(), 1, "Handler should have been called once");
        assert_eq!(
            handled[0],
            serde_json::json!({"action": "approve", "item": "order_123"})
        );
    }
}
