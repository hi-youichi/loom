//! Public Pregel runtime entrypoints.
//!
//! [`PregelRuntime`] is the main user-facing wrapper around a validated
//! [`PregelGraph`]. It provides builder-style configuration for persistence,
//! task caching, managed runtime values, and cancellation, then exposes a small
//! set of execution and introspection APIs.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;

use tokio_util::sync::CancellationToken;
use loom_graph_core::GraphError;
use checkpoint::{
    Checkpoint, CheckpointError, CheckpointListItem, CheckpointSource, Checkpointer,
    RunnableConfig, Store,
};
use crate::algo::{
    normalize_pending_sends, normalize_pending_writes, restore_channels_from_checkpoint,
    task_cache_key,
};
use crate::cache::{CachedTaskWrites, PregelTaskCache};
use crate::config::PregelConfig;
use crate::graph_view::PregelGraphView;
use crate::loop_state::PregelLoop;
use crate::node::{PregelGraph, PregelNodeContext};
use crate::replay::{ReplayMode, ReplayRequest, ReplayResult};
use crate::runner::PregelRunner;
use crate::state::{BulkStateUpdateRequest, PregelStateSnapshot, StateUpdateRequest};
use crate::subgraph::{PregelSubgraphEntry, SubgraphInvocation, SubgraphResult};
use crate::types::{ChannelValue, ManagedValues, ReservedWrite, ResumeMap};
use stream_event::{StreamEvent, StreamMode};

/// Stream handle for a Pregel run.
pub struct PregelStream {
    /// Stream of runtime events emitted during the run.
    ///
    /// The current implementation keeps this channel for API compatibility, but
    /// it may produce no intermediate events depending on the configured stream
    /// mode and runtime capabilities.
    pub events: ReceiverStream<StreamEvent<ChannelValue>>,
    /// Join handle that resolves to the final output or execution error.
    ///
    /// Consumers should await this handle to learn whether the run completed
    /// successfully, even if they ignore `events`.
    pub completion: JoinHandle<Result<ChannelValue, GraphError>>,
}

struct PendingCheckpointWrite {
    checkpoint: Checkpoint<ChannelValue>,
    completion: JoinHandle<Result<(), GraphError>>,
}

/// Public runtime entrypoint for Pregel graph execution.
///
/// The runtime owns the immutable graph definition plus optional services such
/// as a [`Checkpointer`], task cache, or long-term [`Store`]. Use the `with_*`
/// methods to attach those services before invoking the graph.
#[derive(Clone)]
pub struct PregelRuntime {
    graph: Arc<PregelGraph>,
    checkpointer: Option<Arc<dyn Checkpointer<ChannelValue>>>,
    task_cache: Option<Arc<dyn PregelTaskCache>>,
    managed_values: ManagedValues,
    store: Option<Arc<dyn Store>>,
    cancellation: Option<CancellationToken>,
    config: PregelConfig,
}

impl std::fmt::Debug for PregelRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PregelRuntime")
            .field("graph", &self.graph)
            .field("has_checkpointer", &self.checkpointer.is_some())
            .field("has_task_cache", &self.task_cache.is_some())
            .field("has_store", &self.store.is_some())
            .field("has_cancellation", &self.cancellation.is_some())
            .field("config", &self.config)
            .finish()
    }
}

impl PregelRuntime {
    /// Creates a new runtime for a graph definition.
    ///
    /// The returned runtime has no persistence, cache, store, or cancellation
    /// configured yet.
    pub fn new(graph: PregelGraph) -> Self {
        Self {
            graph: Arc::new(graph),
            checkpointer: None,
            task_cache: None,
            managed_values: ManagedValues::default(),
            store: None,
            cancellation: None,
            config: PregelConfig::default(),
        }
    }

    /// Attaches a checkpointer to the runtime.
    ///
    /// When present, runtime state can be resumed, inspected, replayed, or
    /// forked through checkpoint-aware APIs.
    pub fn with_checkpointer(self, checkpointer: Arc<dyn Checkpointer<ChannelValue>>) -> Self {
        Self {
            checkpointer: Some(checkpointer),
            ..self
        }
    }

    /// Attaches a long-term store to the runtime.
    ///
    /// The store is made available to nodes through their execution context.
    pub fn with_store(self, store: Arc<dyn Store>) -> Self {
        Self {
            store: Some(store),
            ..self
        }
    }

    /// Attaches a task cache for cached-write reuse.
    ///
    /// Cached writes let deterministic tasks skip recomputation when their
    /// cache key matches a previous run.
    pub fn with_task_cache(self, task_cache: Arc<dyn PregelTaskCache>) -> Self {
        Self {
            task_cache: Some(task_cache),
            ..self
        }
    }

    /// Attaches a cancellation handle to the runtime.
    ///
    /// Cancellation is checked while the runtime advances tasks and barriers.
    pub fn with_cancellation(self, cancellation: Option<CancellationToken>) -> Self {
        Self {
            cancellation,
            ..self
        }
    }

    /// Replaces managed runtime values injected into each task.
    ///
    /// Managed values act like runtime-scoped ambient inputs that nodes can
    /// read without modeling them as graph channels.
    pub fn with_managed_values(self, managed_values: ManagedValues) -> Self {
        Self {
            managed_values,
            ..self
        }
    }

    /// Adds or overwrites one managed runtime value.
    pub fn with_managed_value(mut self, key: impl Into<String>, value: ChannelValue) -> Self {
        self.managed_values.insert(key.into(), value);
        self
    }

    /// Replaces the runtime config.
    ///
    /// This controls execution behavior such as interrupts, durability, and
    /// stream mode defaults.
    pub fn with_config(self, config: PregelConfig) -> Self {
        Self { config, ..self }
    }

    /// Returns the graph definition.
    ///
    /// The graph is stored behind an [`Arc`] so runtimes and subgraphs can
    /// share one immutable definition.
    pub fn graph(&self) -> &Arc<PregelGraph> {
        &self.graph
    }

    /// Validates the current graph definition and runtime interrupt config.
    ///
    /// Call this early in tests or setup code when you want structural errors
    /// to fail fast before any execution begins.
    pub fn validate(&self) -> Result<(), GraphError> {
        self.graph.validate_with_config(&self.config)
    }

    /// Returns a stable, serializable view of the graph definition.
    ///
    /// This is useful for tests, debugging, and UIs that need to inspect the
    /// graph without depending on internal runtime types.
    pub fn get_graph(&self) -> Result<PregelGraphView, GraphError> {
        self.validate()?;
        Ok(PregelGraphView::from_graph(self.graph.as_ref()))
    }

    /// Returns a graph view and optionally includes recursively discovered subgraphs.
    ///
    /// When `recurse` is `true`, nested graphs reachable through node-attached
    /// subgraphs are embedded in the result.
    pub fn get_graph_xray(&self, recurse: bool) -> Result<PregelGraphView, GraphError> {
        self.validate()?;
        Ok(PregelGraphView::from_graph_with_subgraphs(
            self.graph.as_ref(),
            recurse,
        ))
    }

    /// Async wrapper for graph export to mirror other Pregel APIs.
    pub async fn aget_graph(&self) -> Result<PregelGraphView, GraphError> {
        self.get_graph()
    }

    /// Async wrapper for recursive graph export.
    pub async fn aget_graph_xray(&self, recurse: bool) -> Result<PregelGraphView, GraphError> {
        self.get_graph_xray(recurse)
    }

    /// Discovers child Pregel runtimes exposed by nodes.
    ///
    /// The returned entries include stable paths so callers can surface nested
    /// graphs in tooling without executing them.
    pub fn get_subgraphs(&self, recurse: bool) -> Result<Vec<PregelSubgraphEntry>, GraphError> {
        self.validate()?;
        let mut entries = Vec::new();
        collect_subgraphs(self, "", recurse, &mut entries);
        entries.sort_by_key(|a| a.path.clone());
        Ok(entries)
    }

    /// Async wrapper for subgraph discovery.
    pub async fn aget_subgraphs(
        &self,
        recurse: bool,
    ) -> Result<Vec<PregelSubgraphEntry>, GraphError> {
        self.get_subgraphs(recurse)
    }

    /// Clears all entries from the configured task cache, if any.
    pub fn clear_cache(&self) -> Result<(), GraphError> {
        if let Some(cache) = &self.task_cache {
            cache.clear();
        }
        Ok(())
    }

    /// Async wrapper for clearing the configured task cache.
    pub async fn aclear_cache(&self) -> Result<(), GraphError> {
        self.clear_cache()
    }

    /// Clears cached writes for the selected node names only.
    pub fn clear_cache_for_nodes(&self, node_names: &[String]) -> Result<(), GraphError> {
        if let Some(cache) = &self.task_cache {
            cache.clear_nodes(node_names);
        }
        Ok(())
    }

    /// Async wrapper for selective cache invalidation.
    pub async fn aclear_cache_for_nodes(&self, node_names: &[String]) -> Result<(), GraphError> {
        self.clear_cache_for_nodes(node_names)
    }

    /// Initializes loop state for a run.
    ///
    /// If a matching checkpoint exists, the loop is restored from persisted
    /// state; otherwise the provided `input` becomes the first checkpoint.
    pub async fn init_loop(
        &self,
        input: ChannelValue,
        config: Option<RunnableConfig>,
    ) -> Result<PregelLoop, GraphError> {
        self.validate()?;
        let config = config.unwrap_or_default();
        validate_checkpointer_config(self.checkpointer.as_ref(), &config)?;
        let mut checkpoint = match &self.checkpointer {
            Some(checkpointer) => match checkpointer.get_tuple(&config).await {
                Ok(Some((checkpoint, _metadata))) => checkpoint,
                Ok(None) => Checkpoint::from_state(input, CheckpointSource::Input, 0),
                Err(error) => return Err(checkpoint_error(error)),
            },
            None => Checkpoint::from_state(input, CheckpointSource::Input, 0),
        };
        normalize_checkpoint_frontier(&mut checkpoint);
        let resume_map = resume_map_from_sources(&config, &checkpoint);
        let pending_interrupts = pending_interrupts_from_checkpoint(&checkpoint);
        let consumed_interrupt_ids = consumed_interrupt_ids(&pending_interrupts, &resume_map);
        let channels = restore_channels_from_checkpoint(&checkpoint, &self.graph);
        let mut loop_state = PregelLoop::new(
            Arc::clone(&self.graph),
            config.checkpoint_ns.clone(),
            checkpoint,
            channels,
            self.config.clone(),
        );
        loop_state.interrupts.pending_resume_values = if consumed_interrupt_ids.is_empty() {
            Vec::new()
        } else {
            resume_values(&resume_map)
        };
        loop_state.interrupts.consumed_interrupt_ids = consumed_interrupt_ids;
        Ok(loop_state)
    }

    /// Runs the graph to completion and returns the surfaced output value.
    ///
    /// Depending on configuration, this may resume from an existing checkpoint
    /// lineage, emit stream events, or terminate early with an interrupt.
    pub async fn invoke(
        &self,
        input: ChannelValue,
        config: Option<RunnableConfig>,
    ) -> Result<ChannelValue, GraphError> {
        self.invoke_inner(input, config, None).await
    }

    async fn invoke_inner(
        &self,
        input: ChannelValue,
        config: Option<RunnableConfig>,
        stream_tx: Option<mpsc::Sender<StreamEvent<ChannelValue>>>,
    ) -> Result<ChannelValue, GraphError> {
        let run_config = config.unwrap_or_default();
        let mut loop_state = self.init_loop(input, Some(run_config.clone())).await?;
        let runner = PregelRunner::new(self.config.retry_policy.clone());
        let resume_map = resume_map_from_sources(&run_config, &loop_state.checkpoint);
        let node_ctx = PregelNodeContext {
            cancellation: self.cancellation.clone(),
            stream_tx: stream_tx.clone(),
            stream_mode: self.config.stream_mode.clone(),
            managed_values: self.managed_values.clone(),
            pending_interrupts: pending_interrupts_from_checkpoint(&loop_state.checkpoint),
            resume_map,
            run_config: run_config.clone(),
            parent_runtime: Some(Arc::new(self.clone())),
            subgraph_links: Default::default(),
            runtime: serde_json::Value::Null,
        };
        let mut inflight_checkpoint = None;

let result = async {
            loop {
                let Some(tasks) = loop_state.tick().await? else {
                    break;
                };
                let tasks = self.attach_cached_writes(tasks, &loop_state.checkpoint, &run_config);
                let outcomes = runner
                    .run_step(tasks, Arc::clone(&loop_state.graph), node_ctx.clone())
                    .await;
                self.store_successful_task_writes(&outcomes, &run_config);
                let updated_node_ids = successful_node_ids(&outcomes);
                loop_state.after_tick(outcomes).await?;
                merge_subgraph_links(&mut loop_state.checkpoint, &node_ctx);
                emit_updates_events(&node_ctx, &updated_node_ids, &loop_state.output()).await;
                emit_values_event(&node_ctx, &loop_state.output()).await;
                match self.config.durability {
                    crate::PregelDurability::Sync => {
                        self.persist_checkpoint(
                            &mut loop_state,
                            &run_config,
                            &node_ctx,
                            CheckpointSource::Loop,
                        )
                        .await?;
                    }
                    crate::PregelDurability::Async => {
                        flush_inflight_checkpoint(&mut inflight_checkpoint, &node_ctx, &run_config)
                            .await?;
                        if let Some(checkpointer) = &self.checkpointer {
                            inflight_checkpoint = Some(spawn_checkpoint_persist(
                                Arc::clone(checkpointer),
                                run_config.clone(),
                                next_checkpoint(&loop_state.checkpoint, CheckpointSource::Loop),
                            ));
                        }
                    }
                    crate::PregelDurability::Exit => {}
                }
            }

            // Emit a final Values event with the loop output before exiting the
            // stream so consumers using `StreamMode::Values` can capture the
            // terminal state. Without this, the consumer only sees Values events
            // emitted during active ticks and would observe `StreamEndedWithoutState`.
            emit_values_event(&node_ctx, &loop_state.output()).await;

            crate::finish_channels(&mut loop_state.channels);

            match self.config.durability {
                crate::PregelDurability::Sync => {}
                crate::PregelDurability::Async => {
                    flush_inflight_checkpoint(&mut inflight_checkpoint, &node_ctx, &run_config)
                        .await?;
                }
                crate::PregelDurability::Exit => {
                    self.persist_checkpoint(
                        &mut loop_state,
                        &run_config,
                        &node_ctx,
                        CheckpointSource::Loop,
                    )
                    .await?;
                }
            }

            Ok::<(), GraphError>(())
        }
        .await;

        match result {
            Ok(()) => Ok(loop_state.final_output()),
            Err(GraphError::Interrupted(interrupt)) => {
                flush_inflight_checkpoint(&mut inflight_checkpoint, &node_ctx, &run_config).await?;
                self.persist_checkpoint(
                    &mut loop_state,
                    &run_config,
                    &node_ctx,
                    CheckpointSource::Loop,
                )
                .await?;
                Err(GraphError::Interrupted(interrupt))
            }
            Err(GraphError::Cancelled) => {
                flush_inflight_checkpoint(&mut inflight_checkpoint, &node_ctx, &run_config).await?;
                if checkpoint_has_recoverable_progress(&loop_state.checkpoint) {
                    self.persist_checkpoint(
                        &mut loop_state,
                        &run_config,
                        &node_ctx,
                        CheckpointSource::Loop,
                    )
                    .await?;
                }
                Err(GraphError::Cancelled)
            }
            Err(GraphError::ExecutionFailed(message)) => {
                flush_inflight_checkpoint(&mut inflight_checkpoint, &node_ctx, &run_config).await?;
                if checkpoint_has_recoverable_progress(&loop_state.checkpoint) {
                    self.persist_checkpoint(
                        &mut loop_state,
                        &run_config,
                        &node_ctx,
                        CheckpointSource::Loop,
                    )
                    .await?;
                }
                Err(GraphError::ExecutionFailed(message))
            }
        }
    }

    /// Starts a streamed run.
    ///
    /// This is the streaming counterpart to [`Self::invoke`]. Consumers can
    /// read `events` opportunistically and must await `completion` for the final
    /// result. The current implementation may emit no intermediate events.
    pub fn stream(&self, input: ChannelValue, config: Option<RunnableConfig>) -> PregelStream {
        let (tx, rx) = mpsc::channel(64);
        let runtime = self.clone();
        let completion =
            tokio::spawn(async move { runtime.invoke_inner(input, config, Some(tx)).await });
        PregelStream {
            events: ReceiverStream::new(rx),
            completion,
        }
    }

    /// Loads the latest checkpoint-backed runtime state.
    ///
    /// Returns `Ok(None)` when no checkpointer is configured or when the
    /// selected run has not produced a checkpoint yet.
    pub async fn get_state(
        &self,
        config: RunnableConfig,
    ) -> Result<Option<PregelStateSnapshot>, GraphError> {
        let Some(checkpointer) = &self.checkpointer else {
            return Ok(None);
        };
        validate_checkpointer_config(Some(checkpointer), &config)?;
        let checkpoint = checkpointer
            .get_tuple(&config)
            .await
            .map_err(checkpoint_error)?
            .map(|(checkpoint, _metadata)| checkpoint);
        Ok(checkpoint.as_ref().map(|checkpoint| {
            let mut checkpoint = checkpoint.clone();
            normalize_checkpoint_frontier(&mut checkpoint);
            PregelStateSnapshot::from_checkpoint(&checkpoint)
        }))
    }

    /// Lists checkpoint history metadata for a run.
    ///
    /// Use `before` and `after` to page within one checkpoint lineage.
    pub async fn get_state_history(
        &self,
        config: RunnableConfig,
        limit: Option<usize>,
        before: Option<&str>,
        after: Option<&str>,
    ) -> Result<Vec<CheckpointListItem>, GraphError> {
        let Some(checkpointer) = &self.checkpointer else {
            return Ok(Vec::new());
        };
        validate_checkpointer_config(Some(checkpointer), &config)?;
        checkpointer
            .list(&config, limit, before, after)
            .await
            .map_err(checkpoint_error)
    }

    /// Applies a synthetic state update through Pregel's write barrier.
    ///
    /// This is useful for externally mutating checkpoint-backed state without
    /// executing a real node.
    pub async fn update_state(
        &self,
        config: RunnableConfig,
        request: StateUpdateRequest,
    ) -> Result<PregelStateSnapshot, GraphError> {
        self.bulk_update_state(
            config,
            BulkStateUpdateRequest {
                updates: vec![request],
            },
        )
        .await
    }

    /// Applies multiple synthetic state updates at one shared barrier.
    ///
    /// All updates are staged against the same checkpoint snapshot before the
    /// barrier is committed.
    pub async fn bulk_update_state(
        &self,
        config: RunnableConfig,
        request: BulkStateUpdateRequest,
    ) -> Result<PregelStateSnapshot, GraphError> {
        validate_checkpointer_config(self.checkpointer.as_ref(), &config)?;
        let mut checkpoint = self
            .load_checkpoint_or_default(&config, serde_json::json!({}))
            .await?;
        let existing_pending_sends = checkpoint.pending_sends.clone();
        let existing_pending_writes = checkpoint.pending_writes.clone();
        let mut channels = restore_channels_from_checkpoint(&checkpoint, &self.graph);
        let tasks = request
            .updates
            .iter()
            .enumerate()
            .map(|(index, update)| {
                synthetic_update_task(
                    index,
                    checkpoint.kernel.step.max(0) as u64,
                    update,
                    &self.graph,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let updated_channels = crate::apply_writes(
            &mut checkpoint,
            &mut channels,
            &tasks,
            &self.graph,
            |current| {
                let next = current.and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) + 1;
                next.to_string()
            },
        );
        let new_pending_sends = std::mem::take(&mut checkpoint.pending_sends);
        let new_pending_writes = std::mem::take(&mut checkpoint.pending_writes);
        checkpoint.pending_sends = existing_pending_sends;
        checkpoint.pending_sends.extend(new_pending_sends);
        checkpoint.pending_writes = existing_pending_writes;
        checkpoint.pending_writes.extend(new_pending_writes);
        normalize_checkpoint_frontier(&mut checkpoint);
        checkpoint.updated_channels = Some(updated_channels);
        checkpoint.kernel.step = checkpoint.kernel.step.max(0) + 1;

        let checkpoint = if self.checkpointer.is_some() {
            self.persist_raw_checkpoint(&config, &checkpoint).await?
        } else {
            checkpoint
        };
        Ok(PregelStateSnapshot::from_checkpoint(&checkpoint))
    }

    /// Inspects, resumes, or forks a checkpoint lineage.
    ///
    /// Returns `Ok(None)` when the requested replay mode requires a checkpointer
    /// but no checkpoint or checkpointer is available.
    pub async fn replay(
        &self,
        config: RunnableConfig,
        request: ReplayRequest,
    ) -> Result<Option<ReplayResult>, GraphError> {
        match request.mode {
            ReplayMode::InspectCheckpoint(checkpoint_id) => {
                let mut replay_config = config.clone();
                if let Some(namespace) = &request.namespace {
                    replay_config.checkpoint_ns = namespace.clone();
                }
                replay_config.checkpoint_id = Some(checkpoint_id);
                Ok(self
                    .get_state(replay_config)
                    .await?
                    .map(|snapshot| ReplayResult {
                        snapshot,
                        forked: false,
                    }))
            }
            ReplayMode::ResumeFromCheckpoint(checkpoint_id) => {
                let mut replay_config = config.clone();
                if let Some(namespace) = &request.namespace {
                    replay_config.checkpoint_ns = namespace.clone();
                }
                replay_config.checkpoint_id = Some(checkpoint_id);
                let Some(checkpointer) = &self.checkpointer else {
                    return Ok(None);
                };
                validate_checkpointer_config(Some(checkpointer), &replay_config)?;
                let Some((mut checkpoint, _metadata)) = checkpointer
                    .get_tuple(&replay_config)
                    .await
                    .map_err(checkpoint_error)?
                else {
                    return Ok(None);
                };
                normalize_checkpoint_frontier(&mut checkpoint);

                self.invoke_inner(
                    checkpoint.channel_values.clone(),
                    Some(replay_config.clone()),
                    None,
                )
                .await?;

                replay_config.checkpoint_id = None;
                Ok(self
                    .get_state(replay_config)
                    .await?
                    .map(|snapshot| ReplayResult {
                        snapshot,
                        forked: false,
                    }))
            }
            ReplayMode::ForkFromCheckpoint(checkpoint_id) => {
                let Some(checkpointer) = &self.checkpointer else {
                    return Ok(None);
                };
                let mut source_config = config.clone();
                if let Some(namespace) = &request.namespace {
                    source_config.checkpoint_ns = namespace.clone();
                }
                validate_checkpointer_config(Some(checkpointer), &source_config)?;
                source_config.checkpoint_id = Some(checkpoint_id.clone());
                let Some((mut checkpoint, _metadata)) = checkpointer
                    .get_tuple(&source_config)
                    .await
                    .map_err(checkpoint_error)?
                else {
                    return Ok(None);
                };
                normalize_checkpoint_frontier(&mut checkpoint);

                let forked =
                    checkpoint.fork_from(source_config.checkpoint_ns.clone(), checkpoint_id);
                let source_children = checkpoint
                    .kernel
                    .children
                    .entry(config.checkpoint_ns.clone())
                    .or_default();
                if !source_children
                    .iter()
                    .any(|existing| existing == &forked.id)
                {
                    source_children.push(forked.id.clone());
                }
                checkpointer
                    .put(&source_config, &checkpoint)
                    .await
                    .map_err(checkpoint_error)?;
                checkpointer
                    .put(&config, &forked)
                    .await
                    .map_err(checkpoint_error)?;
                Ok(Some(ReplayResult {
                    snapshot: PregelStateSnapshot::from_checkpoint(&forked),
                    forked: true,
                }))
            }
        }
    }

    /// Invokes a child Pregel runtime under an isolated checkpoint namespace.
    ///
    /// Parent and child runtimes may share the same underlying checkpointer,
    /// but the child always executes inside its own namespace so its lineage can
    /// be resumed or inspected independently.
    pub async fn invoke_subgraph(
        &self,
        child_runtime: &PregelRuntime,
        config: RunnableConfig,
        invocation: SubgraphInvocation,
    ) -> Result<SubgraphResult, GraphError> {
        self.invoke_subgraph_with_stream(child_runtime, config, invocation, None)
            .await
    }

    pub(crate) async fn invoke_subgraph_with_stream(
        &self,
        child_runtime: &PregelRuntime,
        config: RunnableConfig,
        invocation: SubgraphInvocation,
        stream_tx: Option<mpsc::Sender<StreamEvent<ChannelValue>>>,
    ) -> Result<SubgraphResult, GraphError> {
        let child_runtime = child_runtime
            .clone()
            .with_cancellation(self.cancellation.clone());
        let child_namespace = invocation.child_namespace.clone().0;
        let child_config = RunnableConfig {
            checkpoint_ns: child_namespace.clone(),
            checkpoint_id: None,
            depth: Some(config.depth.unwrap_or(0) + 1),
            ..config.clone()
        };
        let result = match child_runtime
            .invoke_inner(
                invocation.entry_input,
                Some(child_config.clone()),
                stream_tx,
            )
            .await
        {
            Ok(value) => SubgraphResult::Completed(value),
            Err(GraphError::Interrupted(interrupt)) => {
                if let Some(state) = child_runtime.get_state(child_config.clone()).await? {
                    if let Some(mut record) = state.pending_interrupts.into_iter().next() {
                        if record.namespace.is_empty() {
                            record.namespace = child_namespace.clone();
                        }
                        return Ok(SubgraphResult::Interrupted(record));
                    }
                }

                SubgraphResult::Interrupted(crate::InterruptRecord {
                    interrupt_id: interrupt
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("subgraph:{}", invocation.parent_task_id)),
                    namespace: child_namespace.clone(),
                    task_id: invocation.parent_task_id,
                    node_name: "subgraph".to_string(),
                    step: 0,
                    value: interrupt.value,
                })
            }
            Err(GraphError::Cancelled) => SubgraphResult::Cancelled,
            Err(error) => SubgraphResult::Failed(error.to_string()),
        };

        if let Some(state) = child_runtime.get_state(child_config).await? {
            if let Some(checkpoint_id) = invocation.parent_checkpoint_id {
                // Link child -> parent
                if let Some(checkpointer) = &child_runtime.checkpointer {
                    // Update the child's checkpoint to point to the parent
                    let mut child_checkpoint = checkpointer
                        .get_tuple(&RunnableConfig {
                            checkpoint_ns: child_namespace.clone(),
                            checkpoint_id: Some(state.checkpoint_id.clone()),
                            ..config.clone()
                        })
                        .await
                        .map_err(checkpoint_error)?
                        .map(|(cp, _)| cp)
                        .unwrap_or_else(|| {
                            Checkpoint::from_state(
                                serde_json::json!({}),
                                CheckpointSource::Update,
                                0,
                            )
                        });

                    child_checkpoint
                        .kernel
                        .parents
                        .insert(config.checkpoint_ns.clone(), checkpoint_id);
                    checkpointer
                        .put(
                            &RunnableConfig {
                                checkpoint_ns: child_namespace.clone(),
                                ..config.clone()
                            },
                            &child_checkpoint,
                        )
                        .await
                        .map_err(checkpoint_error)?;
                }
            }
        }

        Ok(result)
    }

    async fn persist_checkpoint(
        &self,
        loop_state: &mut PregelLoop,
        config: &RunnableConfig,
        ctx: &PregelNodeContext,
        source: CheckpointSource,
    ) -> Result<(), GraphError> {
        let Some(checkpointer) = &self.checkpointer else {
            return Ok(());
        };
        validate_checkpointer_config(Some(checkpointer), config)?;
        merge_subgraph_links(&mut loop_state.checkpoint, ctx);
        let checkpoint = next_checkpoint(&loop_state.checkpoint, source);
        checkpointer
            .put(config, &checkpoint)
            .await
            .map_err(checkpoint_error)?;
        emit_checkpoint_event(ctx, config, &checkpoint).await;
        loop_state.checkpoint = checkpoint;
        Ok(())
    }

    async fn persist_raw_checkpoint(
        &self,
        config: &RunnableConfig,
        checkpoint: &Checkpoint<ChannelValue>,
    ) -> Result<Checkpoint<ChannelValue>, GraphError> {
        let Some(checkpointer) = &self.checkpointer else {
            return Ok(checkpoint.clone());
        };
        validate_checkpointer_config(Some(checkpointer), config)?;
        let persisted = next_checkpoint(checkpoint, CheckpointSource::Update);
        checkpointer
            .put(config, &persisted)
            .await
            .map_err(checkpoint_error)?;
        Ok(persisted)
    }

    async fn load_checkpoint_or_default(
        &self,
        config: &RunnableConfig,
        fallback_input: ChannelValue,
    ) -> Result<Checkpoint<ChannelValue>, GraphError> {
        validate_checkpointer_config(self.checkpointer.as_ref(), config)?;
        match &self.checkpointer {
            Some(checkpointer) => match checkpointer.get_tuple(config).await {
                Ok(Some((mut checkpoint, _metadata))) => {
                    normalize_checkpoint_frontier(&mut checkpoint);
                    Ok(checkpoint)
                }
                Ok(None) => Ok(Checkpoint::from_state(
                    fallback_input,
                    CheckpointSource::Input,
                    0,
                )),
                Err(error) => Err(checkpoint_error(error)),
            },
            None => Ok(Checkpoint::from_state(
                fallback_input,
                CheckpointSource::Input,
                0,
            )),
        }
    }

    fn attach_cached_writes(
        &self,
        tasks: Vec<crate::PreparedTask>,
        checkpoint: &Checkpoint<ChannelValue>,
        run_config: &RunnableConfig,
    ) -> Vec<crate::PreparedTask> {
        let pending_writes_by_task_id: std::collections::HashMap<
            String,
            Vec<(String, ChannelValue)>,
        > = checkpoint.pending_writes.iter().fold(
            std::collections::HashMap::new(),
            |mut acc, (task_id, channel, value)| {
                acc.entry(task_id.clone())
                    .or_default()
                    .push((channel.clone(), value.clone()));
                acc
            },
        );
        let cache = self.task_cache.as_ref();
        tasks
            .into_iter()
            .map(|mut task| {
                if task.cached_writes.is_empty() {
                    if let Some(writes) = pending_writes_by_task_id.get(&task.id) {
                        task.cached_writes = writes.clone();
                    }
                }
                if task.cached_writes.is_empty() {
                    if let Some(cache) = cache {
                        if let Some(cached) = cache.get(&task_cache_key(&task, run_config)) {
                            task.cached_writes = cached.writes;
                        }
                    }
                }
                task
            })
            .collect()
    }

    fn store_successful_task_writes(
        &self,
        outcomes: &[crate::TaskOutcome],
        run_config: &RunnableConfig,
    ) {
        let Some(cache) = &self.task_cache else {
            return;
        };
        for outcome in outcomes {
            let crate::TaskOutcome::Success { task } = outcome else {
                continue;
            };
            if !task.prepared.cached_writes.is_empty() {
                continue;
            }
            let cacheable_writes: Vec<_> = task
                .writes
                .iter()
                .filter(|(ch, _)| !is_reserved_control_write(ch))
                .cloned()
                .collect();
            if cacheable_writes.is_empty() {
                continue;
            }
            cache.put(
                task_cache_key(&task.prepared, run_config),
                CachedTaskWrites {
                    task_id: task.prepared.id.clone(),
                    writes: cacheable_writes,
                },
            );
        }
    }
}

fn collect_subgraphs(
    runtime: &PregelRuntime,
    prefix: &str,
    recurse: bool,
    entries: &mut Vec<PregelSubgraphEntry>,
) {
    for (node_name, node) in &runtime.graph.nodes {
        for subgraph in node.subgraphs() {
            let path = if prefix.is_empty() {
                format!("{node_name}/{}", subgraph.name)
            } else {
                format!("{prefix}/{node_name}/{}", subgraph.name)
            };
            let child_runtime = (*subgraph.runtime).clone();
            entries.push(PregelSubgraphEntry {
                path: path.clone(),
                runtime: child_runtime.clone(),
            });
            if recurse {
                collect_subgraphs(&child_runtime, &path, true, entries);
            }
        }
    }
}

fn is_reserved_control_write(channel: &str) -> bool {
    matches!(
        channel,
        "__interrupt__" | "__error__" | "__return__" | "__no_writes__"
    )
}

async fn emit_values_event(ctx: &PregelNodeContext, state: &ChannelValue) {
    if !(ctx.stream_mode.contains(&StreamMode::Values)
        || ctx.stream_mode.contains(&StreamMode::Debug))
    {
        return;
    }
    if let Some(tx) = &ctx.stream_tx {
        let _ = tx.send(StreamEvent::Values(state.clone())).await;
    }
}

async fn emit_updates_events(ctx: &PregelNodeContext, node_ids: &[String], state: &ChannelValue) {
    if !(ctx.stream_mode.contains(&StreamMode::Updates)
        || ctx.stream_mode.contains(&StreamMode::Debug))
    {
        return;
    }
    let Some(tx) = &ctx.stream_tx else {
        return;
    };
    for node_id in node_ids {
        let _ = tx
            .send(StreamEvent::Updates {
                node_id: node_id.clone(),
                state: state.clone(),
                namespace: if ctx.run_config.checkpoint_ns.is_empty() {
                    None
                } else {
                    Some(ctx.run_config.checkpoint_ns.clone())
                },
            })
            .await;
    }
}

async fn emit_checkpoint_event(
    ctx: &PregelNodeContext,
    config: &RunnableConfig,
    checkpoint: &Checkpoint<ChannelValue>,
) {
    if !(ctx.stream_mode.contains(&StreamMode::Checkpoints)
        || ctx.stream_mode.contains(&StreamMode::Debug))
    {
        return;
    }
    if let Some(tx) = &ctx.stream_tx {
        let _ = tx
            .send(StreamEvent::Checkpoint(stream_event::CheckpointEvent {
                checkpoint_id: checkpoint.id.clone(),
                timestamp: checkpoint.ts.clone(),
                step: checkpoint.kernel.step,
                state: checkpoint.channel_values.clone(),
                thread_id: config.thread_id.clone(),
                checkpoint_ns: if config.checkpoint_ns.is_empty() {
                    None
                } else {
                    Some(config.checkpoint_ns.clone())
                },
            }))
            .await;
    }
}

fn validate_checkpointer_config(
    checkpointer: Option<&Arc<dyn Checkpointer<ChannelValue>>>,
    config: &RunnableConfig,
) -> Result<(), GraphError> {
    if checkpointer.is_some() && config.thread_id.is_none() {
        return Err(checkpoint_error(CheckpointError::ThreadIdRequired));
    }
    Ok(())
}

fn checkpoint_error(error: CheckpointError) -> GraphError {
    GraphError::ExecutionFailed(error.to_string())
}

fn checkpoint_has_recoverable_progress(checkpoint: &Checkpoint<ChannelValue>) -> bool {
    !checkpoint.pending_sends.is_empty()
        || !checkpoint.pending_writes.is_empty()
        || !checkpoint.pending_interrupts.is_empty()
}

fn next_checkpoint(
    current: &Checkpoint<ChannelValue>,
    source: CheckpointSource,
) -> Checkpoint<ChannelValue> {
    let mut checkpoint = Checkpoint::from_state(
        current.channel_values.clone(),
        source,
        current.kernel.step,
    );
    checkpoint.channel_versions = current.channel_versions.clone();
    checkpoint.versions_seen = current.versions_seen.clone();
    checkpoint.updated_channels = current.updated_channels.clone();
    checkpoint.pending_sends = current.pending_sends.clone();
    checkpoint.pending_writes = current.pending_writes.clone();
    checkpoint.pending_interrupts = current.pending_interrupts.clone();
    checkpoint.kernel.parents = current.kernel.parents.clone();
    checkpoint.kernel.children = current.kernel.children.clone();
    checkpoint
}

fn merge_subgraph_links(checkpoint: &mut Checkpoint<ChannelValue>, ctx: &PregelNodeContext) {
    for (namespace, checkpoint_ids) in ctx.subgraph_links() {
        let entry = checkpoint.kernel.children.entry(namespace).or_default();
        for checkpoint_id in checkpoint_ids {
            if !entry.iter().any(|existing| existing == &checkpoint_id) {
                entry.push(checkpoint_id);
            }
        }
    }
}

fn spawn_checkpoint_persist(
    checkpointer: Arc<dyn Checkpointer<ChannelValue>>,
    config: RunnableConfig,
    checkpoint: Checkpoint<ChannelValue>,
) -> PendingCheckpointWrite {
    let checkpoint_for_task = checkpoint.clone();
    let completion = tokio::spawn(async move {
        checkpointer
            .put(&config, &checkpoint_for_task)
            .await
            .map(|_| ())
            .map_err(checkpoint_error)
    });
    PendingCheckpointWrite {
        checkpoint,
        completion,
    }
}

async fn flush_inflight_checkpoint(
    inflight: &mut Option<PendingCheckpointWrite>,
    ctx: &PregelNodeContext,
    config: &RunnableConfig,
) -> Result<(), GraphError> {
    let Some(pending) = inflight.take() else {
        return Ok(());
    };
    let result = pending
        .completion
        .await
        .map_err(|error| GraphError::ExecutionFailed(error.to_string()))?;
    result?;
    emit_checkpoint_event(ctx, config, &pending.checkpoint).await;
    Ok(())
}

fn successful_node_ids(outcomes: &[crate::TaskOutcome]) -> Vec<String> {
    outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            crate::TaskOutcome::Success { task } => Some(task.prepared.node_name.clone()),
            _ => None,
        })
        .collect()
}

fn pending_interrupts_from_checkpoint(
    checkpoint: &Checkpoint<ChannelValue>,
) -> Vec<crate::InterruptRecord> {
    checkpoint
        .pending_interrupts
        .iter()
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect()
}

fn resume_map_from_config(config: &RunnableConfig) -> ResumeMap {
    let values_by_namespace = config.resume_values_by_namespace.clone();
    let values_by_interrupt_id = config.resume_values_by_interrupt_id.clone();
    ResumeMap {
        values_by_namespace,
        values_by_interrupt_id,
    }
}

fn resume_map_from_sources(
    config: &RunnableConfig,
    checkpoint: &Checkpoint<ChannelValue>,
) -> ResumeMap {
    let pending_interrupts = pending_interrupts_from_checkpoint(checkpoint);
    let mut resume_map = resume_map_from_config(config);
    if let Some(value) = &config.resume_value {
        merge_resume_value(
            &mut resume_map,
            value.clone(),
            &pending_interrupts,
            config.checkpoint_ns.as_str(),
        );
    }
    for (_, channel, value) in &checkpoint.pending_writes {
        if channel != ReservedWrite::Resume.as_str() {
            continue;
        }
        merge_resume_write(
            &mut resume_map,
            value,
            &pending_interrupts,
            config.checkpoint_ns.as_str(),
        );
    }
    resume_map
}

fn merge_resume_value(
    resume_map: &mut ResumeMap,
    resume_value: ChannelValue,
    pending_interrupts: &[crate::InterruptRecord],
    checkpoint_namespace: &str,
) {
    if let Some(record) = unambiguous_resume_target(pending_interrupts, checkpoint_namespace) {
        resume_map
            .values_by_interrupt_id
            .entry(record.interrupt_id.clone())
            .or_insert(resume_value.clone());
        resume_map
            .values_by_namespace
            .entry(record.namespace.clone())
            .or_insert(resume_value);
    }
}

fn merge_resume_write(
    resume_map: &mut ResumeMap,
    value: &ChannelValue,
    pending_interrupts: &[crate::InterruptRecord],
    checkpoint_namespace: &str,
) {
    let (resume_value, namespace, interrupt_id) = match value {
        serde_json::Value::Object(map) => (
            map.get("value").cloned().unwrap_or_else(|| value.clone()),
            map.get("namespace")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            map.get("interrupt_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        ),
        _ => (value.clone(), None, None),
    };

    if let Some(interrupt_id) = interrupt_id {
        resume_map
            .values_by_interrupt_id
            .entry(interrupt_id)
            .or_insert(resume_value.clone());
    }

    if let Some(namespace) = namespace {
        resume_map
            .values_by_namespace
            .entry(namespace)
            .or_insert(resume_value.clone());
    }

    merge_resume_value(
        resume_map,
        resume_value,
        pending_interrupts,
        checkpoint_namespace,
    );
}

fn resume_values(resume_map: &ResumeMap) -> Vec<ChannelValue> {
    resume_map
        .values_by_interrupt_id
        .values()
        .cloned()
        .chain(resume_map.values_by_namespace.values().cloned())
        .collect()
}

fn consumed_interrupt_ids(
    pending_interrupts: &[crate::InterruptRecord],
    resume_map: &ResumeMap,
) -> std::collections::HashSet<String> {
    pending_interrupts
        .iter()
        .filter(|record| {
            resume_map
                .values_by_interrupt_id
                .contains_key(&record.interrupt_id)
                || resume_map
                    .values_by_namespace
                    .contains_key(&record.namespace)
        })
        .map(|record| record.interrupt_id.clone())
        .collect()
}

fn unambiguous_resume_target<'a>(
    pending_interrupts: &'a [crate::InterruptRecord],
    checkpoint_namespace: &str,
) -> Option<&'a crate::InterruptRecord> {
    if pending_interrupts.len() == 1 {
        return pending_interrupts.first();
    }
    if checkpoint_namespace.is_empty() {
        return None;
    }
    let mut matching = pending_interrupts
        .iter()
        .filter(|record| record.namespace == checkpoint_namespace);
    let first = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    Some(first)
}

fn normalize_checkpoint_frontier(checkpoint: &mut Checkpoint<ChannelValue>) {
    normalize_pending_sends(&mut checkpoint.pending_sends);
    normalize_pending_writes(&mut checkpoint.pending_writes);
}

fn synthetic_update_task(
    index: usize,
    step: u64,
    request: &StateUpdateRequest,
    graph: &PregelGraph,
) -> Result<crate::ExecutableTask, GraphError> {
    if let Some(node_name) = &request.as_node {
        if !graph.nodes.contains_key(node_name) {
            return Err(GraphError::ExecutionFailed(format!(
                "pregel node not found for state update: {}",
                node_name
            )));
        }
    }
    let writes = update_writes_from_value(&request.values)?;
    Ok(crate::ExecutableTask {
        prepared: crate::PreparedTask {
            id: format!("state-update-{step}-{index}"),
            kind: crate::TaskKind::Pull,
            node_name: request
                .as_node
                .clone()
                .unwrap_or_else(|| "__state_update__".to_string()),
            step,
            triggers: Vec::new(),
            input: request.values.clone(),
            packet_id: None,
            origin_task_id: None,
            cached_writes: Vec::new(),
        },
        writes,
        attempt: 0,
    })
}

fn update_writes_from_value(
    value: &ChannelValue,
) -> Result<Vec<(String, ChannelValue)>, GraphError> {
    let Some(map) = value.as_object() else {
        return Err(GraphError::ExecutionFailed(
            "state update values must be a JSON object".to_string(),
        ));
    };
    Ok(map
        .iter()
        .map(|(channel, value)| (channel.clone(), value.clone()))
        .collect())
}
