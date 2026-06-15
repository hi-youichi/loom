//! Compiled state graph: immutable, supports invoke only.
//!
//! Built by `StateGraph::compile` or `compile_with_checkpointer`. Holds nodes and
//! edge order (derived from explicit edges at compile time), optional checkpointer.
//! When checkpointer is set and config.thread_id is provided, final state is saved after invoke.

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use crate::channels::BoxedStateUpdater;
use crate::conditional::NextEntry;
use crate::interrupt::InterruptHandler;
use crate::logging::{log_graph_complete, log_graph_error, log_graph_start, log_node_complete, log_node_start, log_node_state, log_state_update};
use crate::memory::{Checkpoint, CheckpointSource, Checkpointer, RunnableConfig, Store};
use crate::node_middleware::NodeMiddleware;
use crate::retry::RetryPolicy;
use crate::state_graph::END;
use loom_llm::error::AgentError;
use stream_event::{CheckpointEvent, StreamEvent, StreamMode};
use crate::{Next, Node, RunContext};

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
    /// Optional closure that extracts checkpoint summary from state.
    /// The kernel calls this closure when saving a checkpoint.
    pub(super) metadata_extractor: Option<crate::state_graph::MetadataExtractorFn<S>>,
}

/// Streaming graph execution: event stream plus final completion result.
pub struct GraphStream<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    pub events: ReceiverStream<StreamEvent<S>>,
    pub completion: JoinHandle<Result<(), AgentError>>,
}

impl<S> CompiledStateGraph<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn is_cancelled(run_ctx: Option<&RunContext<S>>) -> bool {
        run_ctx
            .and_then(|ctx| ctx.cancellation.as_ref())
            .is_some_and(CancellationToken::is_cancelled)
    }

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
            if Self::is_cancelled(run_ctx) {
                log_graph_error(&AgentError::Cancelled, config.as_ref().and_then(|c| c.thread_id.as_deref()));
                return Err(AgentError::Cancelled);
            }
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
                            .try_send(StreamEvent::TaskStart {
                                node_id: current_id.clone(),
                                namespace: None,
                            });
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
                            let mut checkpoint =
                                Checkpoint::from_state(state.clone(), CheckpointSource::Update, 0);
                            if let Some(ref extractor) = self.metadata_extractor {
                                checkpoint.kernel.summary = extractor(&checkpoint.channel_values);
                            }
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
                                            .try_send(StreamEvent::Checkpoint(
                                                CheckpointEvent {
                                                    checkpoint_id: checkpoint.id.clone(),
                                                    timestamp: checkpoint.ts.clone(),
                                                    step: checkpoint.kernel.step,
                                                    state: state.clone(),
                                                    thread_id: cfg.thread_id.clone(),
                                                    checkpoint_ns,
                                                },
                                            ));
                                    }
                                }
                            }
                        }
                    }

                    // Call interrupt handler if configured
                    if let Some(handler) = &self.interrupt_handler {
                        let _ = handler.handle_interrupt(interrupt);
                    }

                    // Emit TaskEnd with interrupt info
                    if let Some(ctx) = run_ctx {
                        if let Some(tx) = &ctx.stream_tx {
                            if ctx.stream_mode.contains(&StreamMode::Tasks)
                                || ctx.stream_mode.contains(&StreamMode::Debug)
                            {
                                let _ = tx
                                    .try_send(StreamEvent::TaskEnd {
                                        node_id: current_id.clone(),
                                        result: Err(format!(
                                            "interrupted: {:?}",
                                            interrupt.value
                                        )),
                                        namespace: None,
                                    });
                            }
                        }
                    }

                    // Log and return the interrupt error
                    log_graph_error(&AgentError::Interrupted(interrupt.clone()), config.as_ref().and_then(|c| c.thread_id.as_deref()));
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
                                    .try_send(StreamEvent::TaskEnd {
                                        node_id: current_id.clone(),
                                        result: Err(e.to_string()),
                                        namespace: None,
                                    });
                            }
                        }
                    }
                    log_graph_error(&e, config.as_ref().and_then(|c| c.thread_id.as_deref()));
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
                            .try_send(StreamEvent::TaskEnd {
                                node_id: current_id.clone(),
                                result: Ok(()),
                                namespace: None,
                            });
                    }
                }
            }

            // Log node completion
            log_node_complete(current_id, &next);

            // Apply state update using the configured updater
            self.state_updater.apply_update(state, &new_state);

            // Log state update
            log_state_update(current_id);

            if Self::is_cancelled(run_ctx) {
                log_graph_error(&AgentError::Cancelled, config.as_ref().and_then(|c| c.thread_id.as_deref()));
                return Err(AgentError::Cancelled);
            }

            if let Some(ctx) = run_ctx {
                if let Some(tx) = &ctx.stream_tx {
                    if ctx.stream_mode.contains(&StreamMode::Values) {
                        let _ = tx.try_send(StreamEvent::Values(state.clone()));
                    }
                    if ctx.stream_mode.contains(&StreamMode::Updates) {
                        let _ = tx
                            .try_send(StreamEvent::Updates {
                                node_id: current_id.clone(),
                                state: state.clone(),
                                namespace: None,
                            });
                    }
                }
            }

            let next_id: Option<String> =
                if let Some(NextEntry::Conditional(router)) = self.next_map.get(current_id) {
                    let target = router.resolve(state);
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
                        let mut checkpoint =
                            Checkpoint::from_state(state.clone(), CheckpointSource::Update, 0);
                        if let Some(ref extractor) = self.metadata_extractor {
                            checkpoint.kernel.summary = extractor(&checkpoint.channel_values);
                        }
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
                                        .try_send(StreamEvent::Checkpoint(
                                            CheckpointEvent {
                                                checkpoint_id: checkpoint.id.clone(),
                                                timestamp: checkpoint.ts.clone(),
                                                step: checkpoint.kernel.step,
                                                state: state.clone(),
                                                thread_id: cfg.thread_id.clone(),
                                                checkpoint_ns,
                                            },
                                        ));
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
    /// use loom_graph::RunContext;
    /// use loom_graph::memory::{RunnableConfig, InMemoryStore};
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
        cancellation: Option<CancellationToken>,
        event_forwarder: Option<std::sync::Arc<dyn Fn(stream_event::StreamEvent<S>) + Send + Sync>>,
    ) -> GraphStream<S> {
        let (tx, rx) = mpsc::channel(128);
        let graph = self.clone();
        let mode_set: HashSet<StreamMode> = stream_mode.into();

        let completion = tokio::spawn(async move {
            let mut state = state;
            let mut current_id = match graph.edge_order.first().cloned() {
                Some(id) => id,
                None => return Ok(()),
            };
            let mut run_ctx = RunContext::new(config.clone().unwrap_or_default());
            run_ctx.stream_tx = Some(tx);
            run_ctx.stream_mode = mode_set;
            run_ctx.cancellation = cancellation;
            run_ctx.event_forwarder = event_forwarder;

            graph
                .run_loop_inner(&mut state, &config, &mut current_id, Some(&run_ctx))
                .await
        });

        GraphStream {
            events: ReceiverStream::new(rx),
            completion,
        }
    }

    /// Returns the long-term store if the graph was compiled with `with_store(store)`.
    ///
    /// Nodes can use it for cross-thread memory (e.g. namespace from `config.user_id`).
    pub fn store(&self) -> Option<&Arc<dyn Store>> {
        self.store.as_ref()
    }
}