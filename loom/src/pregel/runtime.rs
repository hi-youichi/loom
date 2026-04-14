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

use crate::cli_run::RunCancellation;
use crate::error::AgentError;
use crate::memory::{
    Checkpoint, CheckpointError, CheckpointListItem, CheckpointSource, Checkpointer,
    RunnableConfig, Store,
};
use crate::pregel::algo::{
    normalize_pending_sends, normalize_pending_writes, restore_channels_from_checkpoint,
    task_cache_key,
};
use crate::pregel::cache::{CachedTaskWrites, PregelTaskCache};
use crate::pregel::config::PregelConfig;
use crate::pregel::graph_view::PregelGraphView;
use crate::pregel::loop_state::PregelLoop;
use crate::pregel::node::{PregelGraph, PregelNodeContext};
use crate::pregel::replay::{ReplayMode, ReplayRequest, ReplayResult};
use crate::pregel::runner::PregelRunner;
use crate::pregel::state::{BulkStateUpdateRequest, PregelStateSnapshot, StateUpdateRequest};
use crate::pregel::subgraph::{PregelSubgraphEntry, SubgraphInvocation, SubgraphResult};
use crate::pregel::types::{ChannelValue, ManagedValues, ReservedWrite, ResumeMap};
use crate::stream::{StreamEvent, StreamMode};

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
    pub completion: JoinHandle<Result<ChannelValue, AgentError>>,
}

struct PendingCheckpointWrite {
    checkpoint: Checkpoint<ChannelValue>,
    completion: JoinHandle<Result<(), AgentError>>,
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
    cancellation: Option<RunCancellation>,
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
    pub fn with_cancellation(self, cancellation: Option<RunCancellation>) -> Self {
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
    pub fn validate(&self) -> Result<(), AgentError> {
        self.graph.validate_with_config(&self.config)
    }

    /// Returns a stable, serializable view of the graph definition.
    ///
    /// This is useful for tests, debugging, and UIs that need to inspect the
    /// graph without depending on internal runtime types.
    pub fn get_graph(&self) -> Result<PregelGraphView, AgentError> {
        self.validate()?;
        Ok(PregelGraphView::from_graph(self.graph.as_ref()))
    }

    /// Returns a graph view and optionally includes recursively discovered subgraphs.
    ///
    /// When `recurse` is `true`, nested graphs reachable through node-attached
    /// subgraphs are embedded in the result.
    pub fn get_graph_xray(&self, recurse: bool) -> Result<PregelGraphView, AgentError> {
        self.validate()?;
        Ok(PregelGraphView::from_graph_with_subgraphs(
            self.graph.as_ref(),
            recurse,
        ))
    }

    /// Async wrapper for graph export to mirror other Pregel APIs.
    pub async fn aget_graph(&self) -> Result<PregelGraphView, AgentError> {
        self.get_graph()
    }

    /// Async wrapper for recursive graph export.
    pub async fn aget_graph_xray(&self, recurse: bool) -> Result<PregelGraphView, AgentError> {
        self.get_graph_xray(recurse)
    }

    /// Discovers child Pregel runtimes exposed by nodes.
    ///
    /// The returned entries include stable paths so callers can surface nested
    /// graphs in tooling without executing them.
    pub fn get_subgraphs(&self, recurse: bool) -> Result<Vec<PregelSubgraphEntry>, AgentError> {
        self.validate()?;
        let mut entries = Vec::new();
        collect_subgraphs(self, "", recurse, &mut entries);
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    /// Async wrapper for subgraph discovery.
    pub async fn aget_subgraphs(
        &self,
        recurse: bool,
    ) -> Result<Vec<PregelSubgraphEntry>, AgentError> {
        self.get_subgraphs(recurse)
    }

    /// Clears all entries from the configured task cache, if any.
    pub fn clear_cache(&self) -> Result<(), AgentError> {
        if let Some(cache) = &self.task_cache {
            cache.clear();
        }
        Ok(())
    }

    /// Async wrapper for clearing the configured task cache.
    pub async fn aclear_cache(&self) -> Result<(), AgentError> {
        self.clear_cache()
    }

    /// Clears cached writes for the selected node names only.
    pub fn clear_cache_for_nodes(&self, node_names: &[String]) -> Result<(), AgentError> {
        if let Some(cache) = &self.task_cache {
            cache.clear_nodes(node_names);
        }
        Ok(())
    }

    /// Async wrapper for selective cache invalidation.
    pub async fn aclear_cache_for_nodes(&self, node_names: &[String]) -> Result<(), AgentError> {
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
    ) -> Result<PregelLoop, AgentError> {
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
    ) -> Result<ChannelValue, AgentError> {
        self.invoke_inner(input, config, None).await
    }

    async fn invoke_inner(
        &self,
        input: ChannelValue,
        config: Option<RunnableConfig>,
        stream_tx: Option<mpsc::Sender<StreamEvent<ChannelValue>>>,
    ) -> Result<ChannelValue, AgentError> {
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
                    crate::pregel::PregelDurability::Sync => {
                        self.persist_checkpoint(
                            &mut loop_state,
                            &run_config,
                            &node_ctx,
                            CheckpointSource::Loop,
                        )
                        .await?;
                    }
                    crate::pregel::PregelDurability::Async => {
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
                    crate::pregel::PregelDurability::Exit => {}
                }
            }

            crate::pregel::finish_channels(&mut loop_state.channels);

            match self.config.durability {
                crate::pregel::PregelDurability::Sync => {}
                crate::pregel::PregelDurability::Async => {
                    flush_inflight_checkpoint(&mut inflight_checkpoint, &node_ctx, &run_config)
                        .await?;
                }
                crate::pregel::PregelDurability::Exit => {
                    self.persist_checkpoint(
                        &mut loop_state,
                        &run_config,
                        &node_ctx,
                        CheckpointSource::Loop,
                    )
                    .await?;
                }
            }

            Ok::<(), AgentError>(())
        }
        .await;

        match result {
            Ok(()) => Ok(loop_state.final_output()),
            Err(AgentError::Interrupted(interrupt)) => {
                flush_inflight_checkpoint(&mut inflight_checkpoint, &node_ctx, &run_config).await?;
                self.persist_checkpoint(
                    &mut loop_state,
                    &run_config,
                    &node_ctx,
                    CheckpointSource::Loop,
                )
                .await?;
                Err(AgentError::Interrupted(interrupt))
            }
            Err(AgentError::Cancelled) => {
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
                Err(AgentError::Cancelled)
            }
            Err(AgentError::ExecutionFailed(message)) => {
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
                Err(AgentError::ExecutionFailed(message))
            }
            Err(AgentError::EmptyLlmResponse { retries }) => {
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
                Err(AgentError::EmptyLlmResponse { retries })
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
    ) -> Result<Option<PregelStateSnapshot>, AgentError> {
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
    ) -> Result<Vec<CheckpointListItem>, AgentError> {
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
    ) -> Result<PregelStateSnapshot, AgentError> {
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
    ) -> Result<PregelStateSnapshot, AgentError> {
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
                    checkpoint.metadata.step.max(0) as u64,
                    update,
                    &self.graph,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let updated_channels = crate::pregel::apply_writes(
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
        checkpoint.metadata.step = checkpoint.metadata.step.max(0) + 1;

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
    ) -> Result<Option<ReplayResult>, AgentError> {
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
                    .metadata
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
    ) -> Result<SubgraphResult, AgentError> {
        self.invoke_subgraph_with_stream(child_runtime, config, invocation, None)
            .await
    }

    pub(crate) async fn invoke_subgraph_with_stream(
        &self,
        child_runtime: &PregelRuntime,
        config: RunnableConfig,
        invocation: SubgraphInvocation,
        stream_tx: Option<mpsc::Sender<StreamEvent<ChannelValue>>>,
    ) -> Result<SubgraphResult, AgentError> {
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
            Err(AgentError::Interrupted(interrupt)) => {
                if let Some(state) = child_runtime.get_state(child_config.clone()).await? {
                    if let Some(mut record) = state.pending_interrupts.into_iter().next() {
                        if record.namespace.is_empty() {
                            record.namespace = child_namespace.clone();
                        }
                        return Ok(SubgraphResult::Interrupted(record));
                    }
                }

                SubgraphResult::Interrupted(crate::pregel::InterruptRecord {
                    interrupt_id: interrupt
                        .0
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("subgraph:{}", invocation.parent_task_id)),
                    namespace: child_namespace.clone(),
                    task_id: invocation.parent_task_id,
                    node_name: "subgraph".to_string(),
                    step: 0,
                    value: interrupt.0.value,
                })
            }
            Err(AgentError::Cancelled) => SubgraphResult::Cancelled,
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
                        .metadata
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
    ) -> Result<(), AgentError> {
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
    ) -> Result<Checkpoint<ChannelValue>, AgentError> {
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
    ) -> Result<Checkpoint<ChannelValue>, AgentError> {
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
        tasks: Vec<crate::pregel::PreparedTask>,
        checkpoint: &Checkpoint<ChannelValue>,
        run_config: &RunnableConfig,
    ) -> Vec<crate::pregel::PreparedTask> {
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
        outcomes: &[crate::pregel::TaskOutcome],
        run_config: &RunnableConfig,
    ) {
        let Some(cache) = &self.task_cache else {
            return;
        };
        for outcome in outcomes {
            let crate::pregel::TaskOutcome::Success { task } = outcome else {
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
            .send(StreamEvent::Checkpoint(crate::stream::CheckpointEvent {
                checkpoint_id: checkpoint.id.clone(),
                timestamp: checkpoint.ts.clone(),
                step: checkpoint.metadata.step,
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
) -> Result<(), AgentError> {
    if checkpointer.is_some() && config.thread_id.is_none() {
        return Err(checkpoint_error(CheckpointError::ThreadIdRequired));
    }
    Ok(())
}

fn checkpoint_error(error: CheckpointError) -> AgentError {
    AgentError::ExecutionFailed(error.to_string())
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
        current.metadata.step,
    );
    checkpoint.channel_versions = current.channel_versions.clone();
    checkpoint.versions_seen = current.versions_seen.clone();
    checkpoint.updated_channels = current.updated_channels.clone();
    checkpoint.pending_sends = current.pending_sends.clone();
    checkpoint.pending_writes = current.pending_writes.clone();
    checkpoint.pending_interrupts = current.pending_interrupts.clone();
    checkpoint.metadata.parents = current.metadata.parents.clone();
    checkpoint.metadata.children = current.metadata.children.clone();
    checkpoint.metadata.summary = current
        .metadata
        .summary
        .clone()
        .or_else(|| extract_summary_from_channel_values(&current.channel_values));
    checkpoint
}

/// Extract the `summary` field from serialized channel values (e.g. ReActState).
/// This bridges the gap between state-level summary and checkpoint metadata,
/// ensuring summaries are persisted even when the metadata was not explicitly set.
fn extract_summary_from_channel_values(channel_values: &serde_json::Value) -> Option<String> {
    channel_values
        .get("summary")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn merge_subgraph_links(checkpoint: &mut Checkpoint<ChannelValue>, ctx: &PregelNodeContext) {
    for (namespace, checkpoint_ids) in ctx.subgraph_links() {
        let entry = checkpoint.metadata.children.entry(namespace).or_default();
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
) -> Result<(), AgentError> {
    let Some(pending) = inflight.take() else {
        return Ok(());
    };
    let result = pending
        .completion
        .await
        .map_err(|error| AgentError::ExecutionFailed(error.to_string()))?;
    result?;
    emit_checkpoint_event(ctx, config, &pending.checkpoint).await;
    Ok(())
}

fn successful_node_ids(outcomes: &[crate::pregel::TaskOutcome]) -> Vec<String> {
    outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            crate::pregel::TaskOutcome::Success { task } => Some(task.prepared.node_name.clone()),
            _ => None,
        })
        .collect()
}

fn pending_interrupts_from_checkpoint(
    checkpoint: &Checkpoint<ChannelValue>,
) -> Vec<crate::pregel::InterruptRecord> {
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
    pending_interrupts: &[crate::pregel::InterruptRecord],
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
    pending_interrupts: &[crate::pregel::InterruptRecord],
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
    pending_interrupts: &[crate::pregel::InterruptRecord],
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
    pending_interrupts: &'a [crate::pregel::InterruptRecord],
    checkpoint_namespace: &str,
) -> Option<&'a crate::pregel::InterruptRecord> {
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
) -> Result<crate::pregel::ExecutableTask, AgentError> {
    if let Some(node_name) = &request.as_node {
        if !graph.nodes.contains_key(node_name) {
            return Err(AgentError::ExecutionFailed(format!(
                "pregel node not found for state update: {}",
                node_name
            )));
        }
    }
    let writes = update_writes_from_value(&request.values)?;
    Ok(crate::pregel::ExecutableTask {
        prepared: crate::pregel::PreparedTask {
            id: format!("state-update-{step}-{index}"),
            kind: crate::pregel::TaskKind::Pull,
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
) -> Result<Vec<(String, ChannelValue)>, AgentError> {
    let Some(map) = value.as_object() else {
        return Err(AgentError::ExecutionFailed(
            "state update values must be a JSON object".to_string(),
        ));
    };
    Ok(map
        .iter()
        .map(|(channel, value)| (channel.clone(), value.clone()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use async_trait::async_trait;
    use serde_json::json;

    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::graph::{GraphInterrupt, Interrupt};
    use crate::memory::{Checkpointer, JsonSerializer, MemorySaver, SqliteSaver};
    use crate::pregel::channel::{ChannelKind, ChannelSpec};
    use crate::pregel::node::{PregelNode, PregelNodeInput, PregelNodeOutput};
    use crate::pregel::{
        CheckpointNamespace, InMemoryPregelTaskCache, ReplayMode, ReplayRequest,
        SubgraphInvocation, SubgraphResult,
    };

    #[derive(Debug)]
    struct EchoNode {
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct RelayNode {
        name: String,
        triggers: Vec<String>,
        reads: Vec<String>,
        input_key: String,
        output_key: String,
    }

    #[derive(Debug)]
    struct InterruptingNode {
        name: String,
        triggers: Vec<String>,
        reads: Vec<String>,
        interrupt_value: serde_json::Value,
    }

    #[derive(Debug)]
    struct CountingNode {
        runs: Arc<AtomicUsize>,
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct CountingRelayNode {
        runs: Arc<AtomicUsize>,
        name: String,
        triggers: Vec<String>,
        reads: Vec<String>,
        input_key: String,
        output_key: String,
    }

    #[derive(Debug)]
    struct ManagedValueNode {
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct ResumeAwareNode {
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct ReservedWriteNode {
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct ReservedInterruptNode {
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct ReservedErrorNode {
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct NamedResumeInterruptNode {
        name: String,
        triggers: Vec<String>,
        reads: Vec<String>,
        interrupt_id: String,
        output_key: String,
    }

    #[derive(Debug)]
    struct SlowResumeInterruptNode {
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct ReturnAndOutputNode {
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct ScheduledSendNode {
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct MultiScheduledSendNode {
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct CountingMultiScheduledSendNode {
        runs: Arc<AtomicUsize>,
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct NoWritesMarkerNode {
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct CancellableNode {
        name: String,
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct SleepingNode {
        name: String,
        triggers: Vec<String>,
        reads: Vec<String>,
        sleep_ms: u64,
    }

    #[derive(Debug)]
    struct StreamingNode {
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[derive(Debug)]
    struct InlineSubgraphNode {
        name: String,
        triggers: Vec<String>,
        reads: Vec<String>,
        child_runtime: Arc<PregelRuntime>,
        base_namespace: CheckpointNamespace,
    }

    #[async_trait]
    impl PregelNode for EchoNode {
        fn name(&self) -> &str {
            "echo"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            let value = input.read_values.get("in").cloned().unwrap_or(json!(null));
            Ok(PregelNodeOutput {
                writes: vec![("out".to_string(), value)],
            })
        }
    }

    #[async_trait]
    impl PregelNode for RelayNode {
        fn name(&self) -> &str {
            &self.name
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            let value = input
                .read_values
                .get(&self.input_key)
                .cloned()
                .unwrap_or(json!(null));
            Ok(PregelNodeOutput {
                writes: vec![(self.output_key.clone(), value)],
            })
        }
    }

    #[async_trait]
    impl PregelNode for InterruptingNode {
        fn name(&self) -> &str {
            &self.name
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            _input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            Err(AgentError::Interrupted(GraphInterrupt(Interrupt::new(
                self.interrupt_value.clone(),
            ))))
        }
    }

    #[async_trait]
    impl PregelNode for CountingNode {
        fn name(&self) -> &str {
            "counting"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            Ok(PregelNodeOutput {
                writes: vec![(
                    "out".to_string(),
                    input.read_values.get("in").cloned().unwrap_or(json!(null)),
                )],
            })
        }
    }

    #[async_trait]
    impl PregelNode for CountingRelayNode {
        fn name(&self) -> &str {
            &self.name
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            let value = input
                .read_values
                .get(&self.input_key)
                .cloned()
                .unwrap_or(json!(null));
            Ok(PregelNodeOutput {
                writes: vec![(self.output_key.clone(), value)],
            })
        }
    }

    #[async_trait]
    impl PregelNode for ManagedValueNode {
        fn name(&self) -> &str {
            "managed"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            Ok(PregelNodeOutput {
                writes: vec![(
                    "out".to_string(),
                    input
                        .managed_values
                        .get("tenant")
                        .cloned()
                        .unwrap_or(json!(null)),
                )],
            })
        }
    }

    #[async_trait]
    impl PregelNode for ResumeAwareNode {
        fn name(&self) -> &str {
            "resume"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            if let Some(value) = input.scratchpad.resume_value {
                return Ok(PregelNodeOutput {
                    writes: vec![("out".to_string(), value)],
                });
            }
            Err(AgentError::Interrupted(GraphInterrupt(Interrupt::with_id(
                json!({"kind": "approval"}),
                "interrupt-resume".to_string(),
            ))))
        }
    }

    #[async_trait]
    impl PregelNode for ReservedWriteNode {
        fn name(&self) -> &str {
            "reserved"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            Ok(PregelNodeOutput {
                writes: vec![(
                    crate::pregel::ReservedWrite::Return.as_str().to_string(),
                    input.read_values.get("in").cloned().unwrap_or(json!(null)),
                )],
            })
        }
    }

    #[async_trait]
    impl PregelNode for ReservedInterruptNode {
        fn name(&self) -> &str {
            "reserved_interrupt"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            if let Some(value) = input.scratchpad.resume_value {
                return Ok(PregelNodeOutput {
                    writes: vec![("out".to_string(), value)],
                });
            }
            Ok(PregelNodeOutput {
                writes: vec![(
                    crate::pregel::ReservedWrite::Interrupt.as_str().to_string(),
                    json!({
                        "kind": "approval_required",
                        "id": "reserved-interrupt-id",
                    }),
                )],
            })
        }
    }

    #[async_trait]
    impl PregelNode for ReservedErrorNode {
        fn name(&self) -> &str {
            "reserved_error"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            _input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            Ok(PregelNodeOutput {
                writes: vec![(
                    crate::pregel::ReservedWrite::Error.as_str().to_string(),
                    json!({
                        "message": "reserved failure",
                        "code": "TEST_ERROR",
                    }),
                )],
            })
        }
    }

    #[async_trait]
    impl PregelNode for NamedResumeInterruptNode {
        fn name(&self) -> &str {
            &self.name
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            if let Some(value) = input.scratchpad.resume_value {
                return Ok(PregelNodeOutput {
                    writes: vec![(self.output_key.clone(), value)],
                });
            }
            Err(AgentError::Interrupted(GraphInterrupt(Interrupt::with_id(
                json!({"kind": "approval"}),
                self.interrupt_id.clone(),
            ))))
        }
    }

    #[async_trait]
    impl PregelNode for SlowResumeInterruptNode {
        fn name(&self) -> &str {
            "slow_resume_interrupt"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            if let Some(value) = input.scratchpad.resume_value {
                return Ok(PregelNodeOutput {
                    writes: vec![("gate".to_string(), value)],
                });
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            Err(AgentError::Interrupted(GraphInterrupt(Interrupt::with_id(
                json!({"kind": "approval"}),
                "slow-interrupt".to_string(),
            ))))
        }
    }

    #[async_trait]
    impl PregelNode for ReturnAndOutputNode {
        fn name(&self) -> &str {
            "return_and_output"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            let value = input.read_values.get("in").cloned().unwrap_or(json!(null));
            Ok(PregelNodeOutput {
                writes: vec![
                    ("out".to_string(), value.clone()),
                    (
                        crate::pregel::ReservedWrite::Return.as_str().to_string(),
                        json!({
                            "kind": "final",
                            "value": value,
                        }),
                    ),
                ],
            })
        }
    }

    #[async_trait]
    impl PregelNode for ScheduledSendNode {
        fn name(&self) -> &str {
            "scheduled_sender"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            Ok(PregelNodeOutput {
                writes: vec![(
                    crate::pregel::ReservedWrite::Scheduled.as_str().to_string(),
                    json!({
                        "id": "scheduled-pkt",
                        "target": "worker",
                        "payload": {
                            "value": input.read_values.get("in").cloned().unwrap_or(json!(null)),
                        },
                    }),
                )],
            })
        }
    }

    #[async_trait]
    impl PregelNode for MultiScheduledSendNode {
        fn name(&self) -> &str {
            "multi_scheduled_sender"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            let value = input.read_values.get("mid").cloned().unwrap_or(json!(null));
            Ok(PregelNodeOutput {
                writes: vec![
                    (
                        crate::pregel::ReservedWrite::Scheduled.as_str().to_string(),
                        json!({
                            "id": "scheduled-pkt-worker",
                            "target": "counting_push",
                            "payload": {
                                "value": value.clone(),
                            },
                        }),
                    ),
                    (
                        crate::pregel::ReservedWrite::Scheduled.as_str().to_string(),
                        json!({
                            "id": "scheduled-pkt-interrupt",
                            "target": "slow_resume_interrupt",
                            "payload": {
                                "value": value,
                            },
                        }),
                    ),
                ],
            })
        }
    }

    #[async_trait]
    impl PregelNode for CountingMultiScheduledSendNode {
        fn name(&self) -> &str {
            "counting_multi_scheduled_sender"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            let value = input.read_values.get("in").cloned().unwrap_or(json!(null));
            Ok(PregelNodeOutput {
                writes: vec![
                    (
                        crate::pregel::ReservedWrite::Scheduled.as_str().to_string(),
                        json!({
                            "id": "scheduled-pkt-a",
                            "target": "worker_a",
                            "payload": {
                                "value": value.clone(),
                            },
                        }),
                    ),
                    (
                        crate::pregel::ReservedWrite::Scheduled.as_str().to_string(),
                        json!({
                            "id": "scheduled-pkt-b",
                            "target": "worker_b",
                            "payload": {
                                "value": value,
                            },
                        }),
                    ),
                ],
            })
        }
    }

    #[async_trait]
    impl PregelNode for NoWritesMarkerNode {
        fn name(&self) -> &str {
            "no_writes"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            _input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            Ok(PregelNodeOutput {
                writes: vec![(
                    crate::pregel::ReservedWrite::NoWrites.as_str().to_string(),
                    json!(true),
                )],
            })
        }
    }

    #[async_trait]
    impl PregelNode for CancellableNode {
        fn name(&self) -> &str {
            &self.name
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            _input: PregelNodeInput,
            ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            let Some(cancellation) = ctx.cancellation.as_ref() else {
                return Ok(PregelNodeOutput::default());
            };
            cancellation.token().cancelled().await;
            Err(AgentError::Cancelled)
        }
    }

    #[async_trait]
    impl PregelNode for SleepingNode {
        fn name(&self) -> &str {
            &self.name
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            _input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            tokio::time::sleep(std::time::Duration::from_millis(self.sleep_ms)).await;
            Ok(PregelNodeOutput::default())
        }
    }

    #[async_trait]
    impl PregelNode for StreamingNode {
        fn name(&self) -> &str {
            "streaming"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            let _ = ctx
                .emit_custom(json!({"kind": "progress", "node": self.name(), "pct": 50}))
                .await;
            let _ = ctx
                .emit_message_chunk(
                    crate::stream::MessageChunk::thinking("thinking"),
                    self.name(),
                )
                .await;
            let _ = ctx.emit_message("done", self.name()).await;

            Ok(PregelNodeOutput {
                writes: vec![(
                    "out".to_string(),
                    input.read_values.get("in").cloned().unwrap_or(json!(null)),
                )],
            })
        }
    }

    #[async_trait]
    impl PregelNode for InlineSubgraphNode {
        fn name(&self) -> &str {
            &self.name
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        fn subgraphs(&self) -> Vec<crate::pregel::PregelSubgraph> {
            vec![crate::pregel::PregelSubgraph {
                name: self.base_namespace.0.clone(),
                runtime: Arc::clone(&self.child_runtime),
            }]
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            let child_namespace = self.base_namespace.child(&input.scratchpad.task_id);
            let value = ctx
                .run_subgraph(
                    self.child_runtime.as_ref(),
                    SubgraphInvocation {
                        parent_task_id: input.scratchpad.task_id.clone(),
                        parent_checkpoint_id: None,
                        child_namespace,
                        entry_input: json!({
                            "in": input.read_values.get("in").cloned().unwrap_or(json!(null)),
                        }),
                    },
                )
                .await?;

            Ok(PregelNodeOutput {
                writes: vec![("out".to_string(), value["out"].clone())],
            })
        }
    }

    #[tokio::test]
    async fn invoke_runs_minimal_single_step_graph() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(graph);
        let output = runtime.invoke(json!({"in": "hello"}), None).await.unwrap();
        assert_eq!(output["out"], json!("hello"));
    }

    #[tokio::test]
    async fn stream_emits_task_value_and_update_events() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(graph).with_config(crate::pregel::PregelConfig {
            stream_mode: vec![StreamMode::Tasks, StreamMode::Values, StreamMode::Updates],
            ..Default::default()
        });

        let mut stream = runtime.stream(json!({"in": "hello"}), None);
        let mut saw_start = false;
        let mut saw_end = false;
        let mut saw_values = false;
        let mut saw_updates = false;

        use tokio_stream::StreamExt;
        while let Some(event) = stream.events.next().await {
            match event {
                StreamEvent::TaskStart { node_id, .. } => {
                    saw_start = true;
                    assert_eq!(node_id, "echo");
                }
                StreamEvent::TaskEnd {
                    node_id, result, ..
                } => {
                    saw_end = true;
                    assert_eq!(node_id, "echo");
                    assert!(result.is_ok());
                }
                StreamEvent::Values(state) => {
                    saw_values = true;
                    assert_eq!(state["out"], json!("hello"));
                }
                StreamEvent::Updates { node_id, state, .. } => {
                    saw_updates = true;
                    assert_eq!(node_id, "echo");
                    assert_eq!(state["out"], json!("hello"));
                }
                _ => {}
            }
        }

        let output = stream.completion.await.unwrap().unwrap();
        assert_eq!(output["out"], json!("hello"));
        assert!(saw_start);
        assert!(saw_end);
        assert!(saw_values);
        assert!(saw_updates);
    }

    #[tokio::test]
    async fn stream_emits_custom_and_message_events_from_pregel_nodes() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(StreamingNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(graph).with_config(crate::pregel::PregelConfig {
            stream_mode: vec![StreamMode::Custom, StreamMode::Messages],
            ..Default::default()
        });

        let mut stream = runtime.stream(json!({"in": "hello"}), None);
        let mut saw_custom = false;
        let mut saw_thinking = false;
        let mut saw_message = false;

        use tokio_stream::StreamExt;
        while let Some(event) = stream.events.next().await {
            match event {
                StreamEvent::Custom(value) => {
                    saw_custom = true;
                    assert_eq!(value["kind"], json!("progress"));
                    assert_eq!(value["node"], json!("streaming"));
                    assert_eq!(value["pct"], json!(50));
                }
                StreamEvent::Messages { chunk, metadata } => {
                    assert_eq!(metadata.loom_node, "streaming");
                    match chunk.kind {
                        crate::stream::MessageChunkKind::Thinking => {
                            saw_thinking = true;
                            assert_eq!(chunk.content, "thinking");
                        }
                        crate::stream::MessageChunkKind::Message => {
                            saw_message = true;
                            assert_eq!(chunk.content, "done");
                        }
                    }
                }
                _ => {}
            }
        }

        let output = stream.completion.await.unwrap().unwrap();
        assert_eq!(output["out"], json!("hello"));
        assert!(saw_custom);
        assert!(saw_thinking);
        assert!(saw_message);
    }

    #[tokio::test]
    async fn parent_node_can_inline_child_subgraph_execution() {
        let checkpointer = Arc::new(MemorySaver::new());

        let mut child_graph = PregelGraph::new();
        child_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();
        let child_runtime =
            Arc::new(PregelRuntime::new(child_graph).with_checkpointer(checkpointer.clone()));

        let mut parent_graph = PregelGraph::new();
        parent_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(InlineSubgraphNode {
                name: "subgraph".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                child_runtime: Arc::clone(&child_runtime),
                base_namespace: CheckpointNamespace("parent/child".to_string()),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();
        let parent_runtime = PregelRuntime::new(parent_graph).with_checkpointer(checkpointer);

        let config = RunnableConfig {
            thread_id: Some("thread-inline-subgraph".to_string()),
            checkpoint_ns: "parent".to_string(),
            ..Default::default()
        };

        let output = parent_runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(output["out"], json!("hello"));

        let parent_state = parent_runtime
            .get_state(config.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(parent_state.channels["out"], json!("hello"));
        let parent_task_id =
            crate::pregel::task_id_for("pregel", "subgraph", 0, crate::pregel::TaskKind::Pull);
        let child_namespace = CheckpointNamespace("parent/child".to_string())
            .child(&parent_task_id)
            .0;

        let child_history = child_runtime
            .get_state_history(
                RunnableConfig {
                    checkpoint_ns: child_namespace.clone(),
                    ..config.clone()
                },
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(child_history.len(), 1);

        let linked_child_ids = parent_state
            .children
            .get(&child_namespace)
            .expect("parent state should expose linked child namespace");
        assert_eq!(linked_child_ids.len(), 1);
        assert_eq!(linked_child_ids[0], child_history[0].checkpoint_id);

        let parent_history = parent_runtime
            .get_state_history(
                RunnableConfig {
                    checkpoint_ns: "parent".to_string(),
                    ..config
                },
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            parent_history[0]
                .metadata
                .children
                .get(&child_namespace)
                .expect("history should persist child link"),
            linked_child_ids
        );
    }

    #[tokio::test]
    async fn parent_run_can_resume_child_subgraph_by_namespace() {
        let checkpointer = Arc::new(MemorySaver::new());

        let mut child_graph = PregelGraph::new();
        child_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ResumeAwareNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();
        let child_runtime =
            Arc::new(PregelRuntime::new(child_graph).with_checkpointer(checkpointer.clone()));

        let mut parent_graph = PregelGraph::new();
        parent_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(InlineSubgraphNode {
                name: "subgraph".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                child_runtime: Arc::clone(&child_runtime),
                base_namespace: CheckpointNamespace("parent/child".to_string()),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();
        let parent_runtime = PregelRuntime::new(parent_graph).with_checkpointer(checkpointer);

        let config = RunnableConfig {
            thread_id: Some("thread-inline-subgraph-resume".to_string()),
            checkpoint_ns: "parent".to_string(),
            ..Default::default()
        };

        let first = parent_runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let parent_state = parent_runtime
            .get_state(config.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(parent_state.pending_interrupts.len(), 1);
        let child_namespace = parent_state.pending_interrupts[0].namespace.clone();
        assert!(child_namespace.starts_with("parent/child/"));

        let resumed = parent_runtime
            .invoke(
                json!({"in": "hello"}),
                Some(RunnableConfig {
                    resume_values_by_namespace: [(child_namespace.clone(), json!("approved"))]
                        .into_iter()
                        .collect(),
                    ..config.clone()
                }),
            )
            .await
            .unwrap();
        assert_eq!(resumed["out"], json!("approved"));

        let final_parent_state = parent_runtime
            .get_state(config.clone())
            .await
            .unwrap()
            .unwrap();
        assert!(final_parent_state.pending_interrupts.is_empty());
        assert_eq!(final_parent_state.channels["out"], json!("approved"));

        let child_history = child_runtime
            .get_state_history(
                RunnableConfig {
                    checkpoint_ns: child_namespace,
                    ..config
                },
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert!(child_history.len() >= 2);

        let mut expected_child_ids: Vec<String> = child_history
            .iter()
            .map(|item| item.checkpoint_id.clone())
            .collect();
        expected_child_ids.sort();
        expected_child_ids.dedup();

        let mut actual_child_ids = final_parent_state
            .children
            .get(
                final_parent_state
                    .children
                    .keys()
                    .find(|ns| ns.starts_with("parent/child/"))
                    .expect("parent state should contain linked child namespace"),
            )
            .expect("linked child checkpoints")
            .clone();
        actual_child_ids.sort();
        actual_child_ids.dedup();

        assert_eq!(actual_child_ids, expected_child_ids);
    }

    #[tokio::test]
    async fn invoke_persists_checkpoint_and_exposes_state_history() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph)
            .with_checkpointer(checkpointer)
            .with_config(crate::pregel::PregelConfig {
                durability: crate::pregel::PregelDurability::Async,
                ..Default::default()
            });
        let config = RunnableConfig {
            thread_id: Some("thread-1".to_string()),
            ..Default::default()
        };

        let output = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(output["out"], json!("hello"));

        let state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(state.channels["out"], json!("hello"));
        assert_eq!(state.step, 1);

        let history = runtime
            .get_state_history(config, None, None, None)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].metadata.step, 1);
    }

    #[tokio::test]
    async fn stream_emits_checkpoint_events_when_enabled() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph)
            .with_checkpointer(checkpointer)
            .with_config(crate::pregel::PregelConfig {
                stream_mode: vec![StreamMode::Checkpoints],
                ..Default::default()
            });
        let config = RunnableConfig {
            thread_id: Some("thread-2".to_string()),
            ..Default::default()
        };

        let mut stream = runtime.stream(json!({"in": "hello"}), Some(config));
        let mut saw_checkpoint = false;

        use tokio_stream::StreamExt;
        while let Some(event) = stream.events.next().await {
            if let StreamEvent::Checkpoint(event) = event {
                saw_checkpoint = true;
                assert_eq!(event.thread_id.as_deref(), Some("thread-2"));
                assert_eq!(event.state["out"], json!("hello"));
            }
        }

        let output = stream.completion.await.unwrap().unwrap();
        assert_eq!(output["out"], json!("hello"));
        assert!(saw_checkpoint);
    }

    #[tokio::test]
    async fn exit_durability_flushes_only_final_checkpoint() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("mid", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(RelayNode {
                name: "first".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                input_key: "in".to_string(),
                output_key: "mid".to_string(),
            }))
            .add_node(Arc::new(RelayNode {
                name: "second".to_string(),
                triggers: vec!["mid".to_string()],
                reads: vec!["mid".to_string()],
                input_key: "mid".to_string(),
                output_key: "out".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph)
            .with_checkpointer(checkpointer)
            .with_config(crate::pregel::PregelConfig {
                durability: crate::pregel::PregelDurability::Exit,
                ..Default::default()
            });
        let config = RunnableConfig {
            thread_id: Some("thread-exit".to_string()),
            ..Default::default()
        };

        let output = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(output["out"], json!("hello"));

        let history = runtime
            .get_state_history(config.clone(), None, None, None)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].metadata.step, 2);

        let state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(state.step, 2);
        assert_eq!(state.channels["mid"], json!("hello"));
        assert_eq!(state.channels["out"], json!("hello"));
    }

    #[tokio::test]
    async fn async_durability_persists_each_completed_step() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("mid", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(RelayNode {
                name: "first".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                input_key: "in".to_string(),
                output_key: "mid".to_string(),
            }))
            .add_node(Arc::new(RelayNode {
                name: "second".to_string(),
                triggers: vec!["mid".to_string()],
                reads: vec!["mid".to_string()],
                input_key: "mid".to_string(),
                output_key: "out".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph)
            .with_checkpointer(checkpointer)
            .with_config(crate::pregel::PregelConfig {
                durability: crate::pregel::PregelDurability::Async,
                ..Default::default()
            });
        let config = RunnableConfig {
            thread_id: Some("thread-async".to_string()),
            ..Default::default()
        };

        let output = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(output["out"], json!("hello"));

        let history = runtime
            .get_state_history(config.clone(), None, None, None)
            .await
            .unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].metadata.step, 1);
        assert_eq!(history[1].metadata.step, 2);

        let state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(state.step, 2);
        assert_eq!(state.channels["out"], json!("hello"));
    }

    #[tokio::test]
    async fn invoke_interrupt_before_persists_checkpoint() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph)
            .with_checkpointer(checkpointer)
            .with_config(crate::pregel::PregelConfig {
                interrupt_before: vec!["echo".to_string()],
                ..Default::default()
            });
        let config = RunnableConfig {
            thread_id: Some("thread-before".to_string()),
            ..Default::default()
        };

        let result = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(result, Err(AgentError::Interrupted(_))));

        let state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(state.step, 0);
        assert_eq!(state.channels["in"], json!("hello"));
        assert!(state
            .channels
            .get("out")
            .unwrap_or(&serde_json::Value::Null)
            .is_null());

        let history = runtime
            .get_state_history(config, None, None, None)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].metadata.step, 0);
    }

    #[tokio::test]
    async fn invoke_interrupt_after_persists_last_committed_checkpoint() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph)
            .with_checkpointer(checkpointer)
            .with_config(crate::pregel::PregelConfig {
                interrupt_after: vec!["echo".to_string()],
                ..Default::default()
            });
        let config = RunnableConfig {
            thread_id: Some("thread-after".to_string()),
            ..Default::default()
        };

        let result = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(result, Err(AgentError::Interrupted(_))));

        let state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(state.step, 0);
        assert_eq!(state.channels["in"], json!("hello"));
        assert!(state
            .channels
            .get("out")
            .unwrap_or(&serde_json::Value::Null)
            .is_null());

        let history = runtime
            .get_state_history(config, None, None, None)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].metadata.step, 0);
    }

    #[tokio::test]
    async fn node_interrupt_persists_checkpoint_and_propagates_error() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(InterruptingNode {
                name: "interrupt".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                interrupt_value: json!({"action": "approve"}),
            }))
            .set_input_channels(vec!["in".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-node-interrupt".to_string()),
            ..Default::default()
        };

        let result = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(result, Err(AgentError::Interrupted(_))));

        let state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(state.step, 0);
        assert_eq!(state.channels["in"], json!("hello"));

        let history = runtime
            .get_state_history(config, None, None, None)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
    }

    #[tokio::test]
    async fn update_state_persists_synthetic_channel_write() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("counter", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["counter".to_string()],
                reads: vec!["counter".to_string()],
            }))
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-update-state".to_string()),
            ..Default::default()
        };

        let state = runtime
            .update_state(
                config.clone(),
                StateUpdateRequest {
                    as_node: None,
                    values: json!({"counter": 1}),
                },
            )
            .await
            .unwrap();
        assert_eq!(state.channels["counter"], json!(1));
        assert_eq!(state.step, 1);

        let persisted = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(persisted.channels["counter"], json!(1));
        assert_eq!(persisted.step, 1);
    }

    #[tokio::test]
    async fn bulk_update_state_commits_multiple_updates_in_one_barrier() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("left", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("right", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["left".to_string()],
                reads: vec!["left".to_string()],
            }))
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-bulk-update".to_string()),
            ..Default::default()
        };

        let state = runtime
            .bulk_update_state(
                config.clone(),
                BulkStateUpdateRequest {
                    updates: vec![
                        StateUpdateRequest {
                            as_node: None,
                            values: json!({"left": "a"}),
                        },
                        StateUpdateRequest {
                            as_node: None,
                            values: json!({"right": "b"}),
                        },
                    ],
                },
            )
            .await
            .unwrap();
        assert_eq!(state.channels["left"], json!("a"));
        assert_eq!(state.channels["right"], json!("b"));
        assert_eq!(state.step, 1);

        let history = runtime
            .get_state_history(config.clone(), None, None, None)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].metadata.step, 1);

        let persisted = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(persisted.channels["left"], json!("a"));
        assert_eq!(persisted.channels["right"], json!("b"));
    }

    #[tokio::test]
    async fn bulk_update_state_preserves_existing_pending_frontier_records() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("counter", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel(
                crate::pregel::TASKS_CHANNEL,
                ChannelSpec::new(ChannelKind::Tasks),
            )
            .add_node(Arc::new(EchoNode {
                triggers: vec!["counter".to_string()],
                reads: vec!["counter".to_string()],
            }))
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer.clone());
        let config = RunnableConfig {
            thread_id: Some("thread-bulk-update-frontier".to_string()),
            ..Default::default()
        };

        let mut checkpoint = crate::memory::Checkpoint::from_state(
            json!({"counter": 0}),
            crate::memory::CheckpointSource::Loop,
            1,
        );
        checkpoint.pending_sends.push((
            "task-queued".to_string(),
            crate::pregel::TASKS_CHANNEL.to_string(),
            json!({
                "id": "pkt-queued",
                "target": "worker",
                "payload": {"value": 1},
                "origin_step": 1
            }),
        ));
        checkpoint.pending_writes.push((
            "task-return".to_string(),
            crate::pregel::ReservedWrite::Return.as_str().to_string(),
            json!("done"),
        ));
        checkpointer.put(&config, &checkpoint).await.unwrap();

        let state = runtime
            .bulk_update_state(
                config.clone(),
                BulkStateUpdateRequest {
                    updates: vec![StateUpdateRequest {
                        as_node: None,
                        values: json!({"counter": 1}),
                    }],
                },
            )
            .await
            .unwrap();

        assert_eq!(state.channels["counter"], json!(1));
        assert_eq!(state.pending_sends.len(), 1);
        assert_eq!(state.pending_sends[0].1, crate::pregel::TASKS_CHANNEL);
        assert_eq!(state.pending_writes.len(), 1);
        assert_eq!(
            state.pending_writes[0].1,
            crate::pregel::ReservedWrite::Return.as_str()
        );

        let persisted = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(persisted.pending_sends.len(), 1);
        assert_eq!(persisted.pending_writes.len(), 1);
    }

    #[tokio::test]
    async fn successful_commit_preserves_unconsumed_pending_sends() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel(
                crate::pregel::TASKS_CHANNEL,
                ChannelSpec::new(ChannelKind::Tasks),
            )
            .add_node(Arc::new(CountingRelayNode {
                runs: Arc::new(AtomicUsize::new(0)),
                name: "worker".to_string(),
                triggers: vec![crate::pregel::TASKS_CHANNEL.to_string()],
                reads: vec!["value".to_string()],
                input_key: "value".to_string(),
                output_key: "out".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer.clone());
        let config = RunnableConfig {
            thread_id: Some("thread-preserve-unconsumed-sends".to_string()),
            ..Default::default()
        };

        let mut checkpoint = crate::memory::Checkpoint::from_state(
            json!({}),
            crate::memory::CheckpointSource::Loop,
            1,
        );
        checkpoint.pending_sends.push((
            "task-worker".to_string(),
            crate::pregel::TASKS_CHANNEL.to_string(),
            json!({
                "id": "pkt-worker",
                "target": "worker",
                "payload": {"value": "hello"},
                "origin_step": 1
            }),
        ));
        checkpoint.pending_sends.push((
            "task-invalid".to_string(),
            crate::pregel::TASKS_CHANNEL.to_string(),
            json!({
                "id": "pkt-invalid",
                "target": "missing-node",
                "payload": {"value": "ignored"},
                "origin_step": 1
            }),
        ));
        checkpointer.put(&config, &checkpoint).await.unwrap();

        let output = runtime
            .invoke(json!({}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(output["out"], json!("hello"));

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(final_state.pending_sends.len(), 1);
        assert_eq!(final_state.pending_sends[0].0, "task-invalid");
    }

    #[tokio::test]
    async fn successful_commit_preserves_unconsumed_pending_writes() {
        let runs = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(CountingNode {
                runs: Arc::clone(&runs),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer.clone());
        let config = RunnableConfig {
            thread_id: Some("thread-preserve-unconsumed-writes".to_string()),
            ..Default::default()
        };

        let current_task_id =
            crate::pregel::task_id_for("pregel", "counting", 1, crate::pregel::TaskKind::Pull);
        let mut checkpoint = crate::memory::Checkpoint::from_state(
            json!({"in": "hello"}),
            crate::memory::CheckpointSource::Loop,
            1,
        );
        checkpoint.updated_channels = Some(vec!["in".to_string()]);
        checkpoint
            .pending_writes
            .push((current_task_id, "out".to_string(), json!("hello")));
        checkpoint.pending_writes.push((
            "other-task".to_string(),
            crate::pregel::ReservedWrite::NoWrites.as_str().to_string(),
            json!(true),
        ));
        checkpointer.put(&config, &checkpoint).await.unwrap();

        let output = runtime
            .invoke(json!({"in": "ignored"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(output["out"], json!("hello"));
        assert_eq!(runs.load(Ordering::SeqCst), 0);

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(final_state.pending_writes.len(), 1);
        assert_eq!(
            final_state.pending_writes[0].1,
            crate::pregel::ReservedWrite::NoWrites.as_str()
        );
    }

    #[tokio::test]
    async fn task_cache_reuses_cached_writes_for_identical_invoke() {
        let runs = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(CountingNode {
                runs: Arc::clone(&runs),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime =
            PregelRuntime::new(graph).with_task_cache(Arc::new(InMemoryPregelTaskCache::new()));

        let first = runtime.invoke(json!({"in": "hello"}), None).await.unwrap();
        let second = runtime.invoke(json!({"in": "hello"}), None).await.unwrap();

        assert_eq!(first["out"], json!("hello"));
        assert_eq!(second["out"], json!("hello"));
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn duplicate_pending_writes_from_checkpoint_are_normalized_before_replay() {
        let runs = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel(
                "out",
                ChannelSpec::new(ChannelKind::Topic { accumulate: true }),
            )
            .add_node(Arc::new(CountingNode {
                runs: Arc::clone(&runs),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer.clone());
        let config = RunnableConfig {
            thread_id: Some("thread-duplicate-pending-writes".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        let task_id =
            crate::pregel::task_id_for("pregel", "counting", 0, crate::pregel::TaskKind::Pull);
        let mut checkpoint = crate::memory::Checkpoint::from_state(
            json!({"in": "hello"}),
            crate::memory::CheckpointSource::Loop,
            0,
        );
        checkpoint.updated_channels = Some(vec!["in".to_string()]);
        checkpoint
            .pending_writes
            .push((task_id.clone(), "out".to_string(), json!("hello")));
        checkpoint
            .pending_writes
            .push((task_id, "out".to_string(), json!("hello")));
        checkpointer.put(&config, &checkpoint).await.unwrap();

        let output = runtime
            .invoke(json!({"in": "ignored"}), Some(config.clone()))
            .await
            .unwrap();

        assert_eq!(output["out"], json!(["hello"]));
        assert_eq!(runs.load(Ordering::SeqCst), 0);

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(final_state.channels["out"], json!(["hello"]));
    }

    #[tokio::test]
    async fn replay_can_inspect_and_fork_checkpoint() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-replay".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        let history = runtime
            .get_state_history(config.clone(), None, None, None)
            .await
            .unwrap();
        let checkpoint_id = history[0].checkpoint_id.clone();

        let inspected = runtime
            .replay(
                config.clone(),
                ReplayRequest {
                    mode: ReplayMode::InspectCheckpoint(checkpoint_id.clone()),
                    namespace: Some("main".to_string()),
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(inspected.snapshot.channels["out"], json!("hello"));
        assert!(!inspected.forked);

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let forked = runtime
            .replay(
                RunnableConfig {
                    checkpoint_ns: "fork".to_string(),
                    ..config.clone()
                },
                ReplayRequest {
                    mode: ReplayMode::ForkFromCheckpoint(checkpoint_id.clone()),
                    namespace: Some("main".to_string()),
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert!(forked.forked);
        assert_ne!(forked.snapshot.checkpoint_id, checkpoint_id);
        assert_eq!(forked.snapshot.channels["out"], json!("hello"));
        assert_eq!(
            forked.snapshot.parents.get("main").map(String::as_str),
            Some(checkpoint_id.as_str())
        );

        let fork_history = runtime
            .get_state_history(
                RunnableConfig {
                    checkpoint_ns: "fork".to_string(),
                    ..config.clone()
                },
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(fork_history.len(), 1);
        assert!(matches!(
            fork_history[0].metadata.source,
            CheckpointSource::Fork
        ));
        assert_ne!(
            fork_history[0].metadata.created_at,
            history[0].metadata.created_at
        );

        let source_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(
            source_state
                .children
                .get("fork")
                .expect("source checkpoint should record fork child"),
            &vec![forked.snapshot.checkpoint_id.clone()]
        );
    }

    #[tokio::test]
    async fn failed_step_with_successful_sibling_persists_recoverable_writes() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(CountingNode {
                runs: Arc::new(AtomicUsize::new(0)),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .add_node(Arc::new(ReservedErrorNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-failure-persists-progress".to_string()),
            ..Default::default()
        };

        let result = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        match result {
            Err(AgentError::ExecutionFailed(message)) => {
                assert_eq!(message, "reserved failure");
            }
            other => panic!("expected reserved failure, got {other:?}"),
        }

        let state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(state.pending_writes.len(), 1);
        assert!(state.channels["out"].is_null());
        assert_eq!(state.pending_writes[0].1, "out");
        assert_eq!(state.pending_writes[0].2, json!("hello"));
    }

    #[tokio::test]
    async fn cancelled_step_with_successful_sibling_persists_recoverable_writes() {
        let cancellation = RunCancellation::new(3);
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(CountingNode {
                runs: Arc::new(AtomicUsize::new(0)),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .add_node(Arc::new(CancellableNode {
                name: "cancel_me".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph)
            .with_checkpointer(checkpointer)
            .with_cancellation(Some(cancellation.clone()));
        let config = RunnableConfig {
            thread_id: Some("thread-cancel-persists-progress".to_string()),
            ..Default::default()
        };

        let runtime_for_task = runtime.clone();
        let config_for_task = config.clone();
        let invoke_task = tokio::spawn(async move {
            runtime_for_task
                .invoke(json!({"in": "hello"}), Some(config_for_task))
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        cancellation.cancel();

        let result = invoke_task.await.unwrap();
        assert!(matches!(result, Err(AgentError::Cancelled)));

        let state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(state.pending_writes.len(), 1);
        assert!(state.channels["out"].is_null());
        assert_eq!(state.pending_writes[0].1, "out");
        assert_eq!(state.pending_writes[0].2, json!("hello"));
    }

    #[tokio::test]
    async fn invoke_with_cancellation_returns_cancelled_without_checkpoint() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(SleepingNode {
                name: "sleep".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                sleep_ms: 5_000,
            }))
            .set_input_channels(vec!["in".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let cancellation = RunCancellation::new(1);
        let runtime = PregelRuntime::new(graph)
            .with_checkpointer(checkpointer.clone())
            .with_cancellation(Some(cancellation.clone()));
        let config = RunnableConfig {
            thread_id: Some("thread-pregel-cancel".to_string()),
            ..Default::default()
        };

        let start = tokio::time::Instant::now();
        let runtime_for_task = runtime.clone();
        let config_for_task = config.clone();
        let invoke_task = tokio::spawn(async move {
            runtime_for_task
                .invoke(json!({"in": "hello"}), Some(config_for_task))
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        cancellation.cancel();

        let result = invoke_task.await.unwrap();
        assert!(matches!(result, Err(AgentError::Cancelled)));
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        assert!(runtime.get_state(config).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn invoke_subgraph_propagates_cancellation_to_child_runtime() {
        let cancellation = RunCancellation::new(2);
        let checkpointer = Arc::new(MemorySaver::new());

        let mut parent_graph = PregelGraph::new();
        parent_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .build_trigger_index();
        let parent_runtime = PregelRuntime::new(parent_graph)
            .with_checkpointer(checkpointer.clone())
            .with_cancellation(Some(cancellation.clone()));

        let mut child_graph = PregelGraph::new();
        child_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(CancellableNode {
                name: "child_cancel".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .build_trigger_index();
        let child_runtime = PregelRuntime::new(child_graph).with_checkpointer(checkpointer);

        let base_config = RunnableConfig {
            thread_id: Some("thread-pregel-subgraph-cancel".to_string()),
            checkpoint_ns: "parent".to_string(),
            ..Default::default()
        };
        let invocation = SubgraphInvocation {
            parent_task_id: "parent-task".to_string(),
            parent_checkpoint_id: None,
            child_namespace: CheckpointNamespace("parent/child-cancel".to_string()),
            entry_input: json!({"in": "child"}),
        };

        let parent_runtime_for_task = parent_runtime.clone();
        let child_runtime_for_task = child_runtime.clone();
        let config_for_task = base_config.clone();
        let invocation_for_task = invocation.clone();
        let task = tokio::spawn(async move {
            parent_runtime_for_task
                .invoke_subgraph(
                    &child_runtime_for_task,
                    config_for_task,
                    invocation_for_task,
                )
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        cancellation.cancel();

        let result = task.await.unwrap().unwrap();
        assert!(matches!(result, SubgraphResult::Cancelled));
    }

    #[tokio::test]
    async fn multiple_interrupts_from_same_step_are_all_persisted() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out_a", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out_b", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(NamedResumeInterruptNode {
                name: "resume_a".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                interrupt_id: "interrupt-a".to_string(),
                output_key: "out_a".to_string(),
            }))
            .add_node(Arc::new(NamedResumeInterruptNode {
                name: "resume_b".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                interrupt_id: "interrupt-b".to_string(),
                output_key: "out_b".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out_a".to_string(), "out_b".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-multi-interrupt".to_string()),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let interrupted_state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(interrupted_state.pending_interrupts.len(), 2);
        let interrupt_ids = interrupted_state
            .pending_interrupts
            .iter()
            .map(|record| record.interrupt_id.clone())
            .collect::<std::collections::HashSet<_>>();
        assert!(interrupt_ids.contains("interrupt-a"));
        assert!(interrupt_ids.contains("interrupt-b"));
    }

    #[tokio::test]
    async fn batch_resume_consumes_multiple_interrupts_in_one_invoke() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out_a", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out_b", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(NamedResumeInterruptNode {
                name: "resume_a".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                interrupt_id: "interrupt-a".to_string(),
                output_key: "out_a".to_string(),
            }))
            .add_node(Arc::new(NamedResumeInterruptNode {
                name: "resume_b".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                interrupt_id: "interrupt-b".to_string(),
                output_key: "out_b".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out_a".to_string(), "out_b".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-batch-resume".to_string()),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let interrupted_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(interrupted_state.pending_interrupts.len(), 2);

        let resumed = runtime
            .invoke(
                json!({"in": "hello"}),
                Some(RunnableConfig {
                    resume_values_by_interrupt_id: interrupted_state
                        .pending_interrupts
                        .iter()
                        .map(|record| {
                            (
                                record.interrupt_id.clone(),
                                json!(record.interrupt_id.clone()),
                            )
                        })
                        .collect(),
                    ..config.clone()
                }),
            )
            .await
            .unwrap();

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert!(final_state.pending_interrupts.is_empty());
        let outputs = [resumed["out_a"].clone(), resumed["out_b"].clone()];
        assert!(outputs.iter().any(|value| value == &json!("interrupt-a")));
        assert!(outputs.iter().any(|value| value == &json!("interrupt-b")));
    }

    #[tokio::test]
    async fn resume_runs_interrupting_task_alongside_other_pending_frontier() {
        let worker_runs = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("resume_trig", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("gate", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel(
                crate::pregel::TASKS_CHANNEL,
                ChannelSpec::new(ChannelKind::Tasks),
            )
            .add_node(Arc::new(NamedResumeInterruptNode {
                name: "resume_a".to_string(),
                triggers: vec!["resume_trig".to_string()],
                reads: vec!["resume_trig".to_string()],
                interrupt_id: "resume-a-interrupt".to_string(),
                output_key: "gate".to_string(),
            }))
            .add_node(Arc::new(CountingRelayNode {
                runs: Arc::clone(&worker_runs),
                name: "worker".to_string(),
                triggers: vec![crate::pregel::TASKS_CHANNEL.to_string()],
                reads: vec!["value".to_string()],
                input_key: "value".to_string(),
                output_key: "out".to_string(),
            }))
            .set_output_channels(vec!["out".to_string(), "gate".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer.clone());
        let config = RunnableConfig {
            thread_id: Some("thread-resume-with-frontier".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        let mut checkpoint = crate::memory::Checkpoint::from_state(
            json!({}),
            crate::memory::CheckpointSource::Loop,
            1,
        );
        checkpoint.pending_interrupts.push(
            serde_json::to_value(crate::pregel::InterruptRecord {
                interrupt_id: "resume-a-interrupt".to_string(),
                namespace: "main".to_string(),
                task_id: "resume-a-task".to_string(),
                node_name: "resume_a".to_string(),
                step: 1,
                value: json!({"kind": "approval"}),
            })
            .unwrap(),
        );
        checkpoint.pending_sends.push((
            "sender-task".to_string(),
            crate::pregel::TASKS_CHANNEL.to_string(),
            json!({
                "id": "pkt-worker",
                "target": "worker",
                "payload": {"value": "hello"},
                "origin_step": 1
            }),
        ));
        checkpointer.put(&config, &checkpoint).await.unwrap();

        let resumed = runtime
            .invoke(
                json!({}),
                Some(RunnableConfig {
                    resume_values_by_interrupt_id: [(
                        "resume-a-interrupt".to_string(),
                        json!("approved"),
                    )]
                    .into_iter()
                    .collect(),
                    ..config.clone()
                }),
            )
            .await
            .unwrap();

        assert_eq!(resumed["out"], json!("hello"));
        assert_eq!(resumed["gate"], json!("approved"));
        assert_eq!(worker_runs.load(Ordering::SeqCst), 1);

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert!(final_state.pending_interrupts.is_empty());
        assert!(final_state.pending_sends.is_empty());
        assert_eq!(final_state.channels["out"], json!("hello"));
        assert_eq!(final_state.channels["gate"], json!("approved"));
    }

    #[tokio::test]
    async fn replay_resume_continues_from_checkpoint() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ResumeAwareNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-replay-resume".to_string()),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let interrupted_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        let resumed = runtime
            .replay(
                RunnableConfig {
                    resume_values_by_interrupt_id: [(
                        interrupted_state.pending_interrupts[0].interrupt_id.clone(),
                        json!("approved"),
                    )]
                    .into_iter()
                    .collect(),
                    ..config.clone()
                },
                ReplayRequest {
                    mode: ReplayMode::ResumeFromCheckpoint(interrupted_state.checkpoint_id.clone()),
                    namespace: None,
                },
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(resumed.snapshot.channels["out"], json!("approved"));
        assert!(resumed.snapshot.pending_interrupts.is_empty());
    }

    #[tokio::test]
    async fn replay_resume_continues_from_sqlite_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("pregel-replay-resume.db");

        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ResumeAwareNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(
            SqliteSaver::<serde_json::Value>::new(&db_path, Arc::new(JsonSerializer)).unwrap(),
        );
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-sqlite-replay-resume".to_string()),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let interrupted_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        let resumed = runtime
            .replay(
                RunnableConfig {
                    resume_values_by_interrupt_id: [(
                        interrupted_state.pending_interrupts[0].interrupt_id.clone(),
                        json!("approved"),
                    )]
                    .into_iter()
                    .collect(),
                    ..config.clone()
                },
                ReplayRequest {
                    mode: ReplayMode::ResumeFromCheckpoint(interrupted_state.checkpoint_id.clone()),
                    namespace: None,
                },
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(resumed.snapshot.channels["out"], json!("approved"));
        assert!(resumed.snapshot.pending_interrupts.is_empty());
    }

    #[tokio::test]
    async fn replay_resume_restores_pull_frontier_from_intermediate_checkpoint() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("mid", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(RelayNode {
                name: "first".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                input_key: "in".to_string(),
                output_key: "mid".to_string(),
            }))
            .add_node(Arc::new(RelayNode {
                name: "second".to_string(),
                triggers: vec!["mid".to_string()],
                reads: vec!["mid".to_string()],
                input_key: "mid".to_string(),
                output_key: "out".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-replay-pull-frontier".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(first["out"], json!("hello"));

        let history = runtime
            .get_state_history(config.clone(), None, None, None)
            .await
            .unwrap();
        let step_one = history
            .iter()
            .find(|item| item.metadata.step == 1)
            .expect("step 1 checkpoint should exist")
            .checkpoint_id
            .clone();
        let intermediate = runtime
            .get_state(RunnableConfig {
                checkpoint_id: Some(step_one.clone()),
                ..config.clone()
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(intermediate.channels["mid"], json!("hello"));
        assert!(!intermediate.updated_channels.is_empty());
        assert!(
            intermediate.channels.get("out").is_none() || intermediate.channels["out"].is_null()
        );

        let resumed = runtime
            .replay(
                config.clone(),
                ReplayRequest {
                    mode: ReplayMode::ResumeFromCheckpoint(step_one),
                    namespace: Some("main".to_string()),
                },
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(resumed.snapshot.channels["out"], json!("hello"));
    }

    #[tokio::test]
    async fn replay_resume_restores_pull_frontier_from_sqlite_intermediate_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("pregel-replay-pull-frontier.db");

        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("mid", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(RelayNode {
                name: "first".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                input_key: "in".to_string(),
                output_key: "mid".to_string(),
            }))
            .add_node(Arc::new(RelayNode {
                name: "second".to_string(),
                triggers: vec!["mid".to_string()],
                reads: vec!["mid".to_string()],
                input_key: "mid".to_string(),
                output_key: "out".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(
            SqliteSaver::<serde_json::Value>::new(&db_path, Arc::new(JsonSerializer)).unwrap(),
        );
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-sqlite-replay-pull-frontier".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(first["out"], json!("hello"));

        let history = runtime
            .get_state_history(config.clone(), None, None, None)
            .await
            .unwrap();
        let step_one = history
            .iter()
            .find(|item| item.metadata.step == 1)
            .expect("step 1 checkpoint should exist")
            .checkpoint_id
            .clone();
        let intermediate = runtime
            .get_state(RunnableConfig {
                checkpoint_id: Some(step_one.clone()),
                ..config.clone()
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(intermediate.channels["mid"], json!("hello"));
        assert!(!intermediate.updated_channels.is_empty());
        assert!(
            intermediate.channels.get("out").is_none() || intermediate.channels["out"].is_null()
        );

        let resumed = runtime
            .replay(
                config.clone(),
                ReplayRequest {
                    mode: ReplayMode::ResumeFromCheckpoint(step_one),
                    namespace: Some("main".to_string()),
                },
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(resumed.snapshot.channels["out"], json!("hello"));
    }

    #[tokio::test]
    async fn replay_resume_restores_pending_sends_from_intermediate_checkpoint() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel(
                crate::pregel::TASKS_CHANNEL,
                ChannelSpec::new(ChannelKind::Tasks),
            )
            .add_node(Arc::new(ScheduledSendNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .add_node(Arc::new(RelayNode {
                name: "worker".to_string(),
                triggers: vec![crate::pregel::TASKS_CHANNEL.to_string()],
                reads: vec!["value".to_string()],
                input_key: "value".to_string(),
                output_key: "out".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph)
            .with_checkpointer(checkpointer)
            .with_config(crate::pregel::PregelConfig {
                durability: crate::pregel::PregelDurability::Async,
                ..Default::default()
            });
        let config = RunnableConfig {
            thread_id: Some("thread-replay-push-frontier".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(first["out"], json!("hello"));

        let history = runtime
            .get_state_history(config.clone(), None, None, None)
            .await
            .unwrap();
        let step_one = history
            .iter()
            .find(|item| item.metadata.step == 1)
            .expect("step 1 checkpoint should exist")
            .checkpoint_id
            .clone();
        let intermediate = runtime
            .get_state(RunnableConfig {
                checkpoint_id: Some(step_one.clone()),
                ..config.clone()
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(intermediate.pending_sends.len(), 1);

        let resumed = runtime
            .replay(
                config.clone(),
                ReplayRequest {
                    mode: ReplayMode::ResumeFromCheckpoint(step_one),
                    namespace: Some("main".to_string()),
                },
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(resumed.snapshot.channels["out"], json!("hello"));
        assert!(resumed.snapshot.pending_sends.is_empty());
    }

    #[tokio::test]
    async fn reserved_writes_are_persisted_in_state_snapshot() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ReservedWriteNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-reserved-writes".to_string()),
            ..Default::default()
        };

        runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();

        let state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(state.pending_writes.len(), 1);
        assert_eq!(
            state.pending_writes[0].1,
            crate::pregel::ReservedWrite::Return.as_str()
        );
        assert_eq!(state.pending_writes[0].2, json!("hello"));
    }

    #[tokio::test]
    async fn reserved_interrupt_writes_raise_resumable_interrupt() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ReservedInterruptNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-reserved-interrupt".to_string()),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let interrupted_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(interrupted_state.pending_interrupts.len(), 1);
        assert_eq!(
            interrupted_state.pending_interrupts[0].interrupt_id,
            "reserved-interrupt-id"
        );

        let resumed = runtime
            .invoke(
                json!({"in": "hello"}),
                Some(RunnableConfig {
                    resume_values_by_interrupt_id: [(
                        interrupted_state.pending_interrupts[0].interrupt_id.clone(),
                        json!("approved"),
                    )]
                    .into_iter()
                    .collect(),
                    ..config.clone()
                }),
            )
            .await
            .unwrap();
        assert_eq!(resumed["out"], json!("approved"));
    }

    #[tokio::test]
    async fn reserved_error_writes_raise_execution_failed() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ReservedErrorNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(graph);
        let result = runtime.invoke(json!({"in": "hello"}), None).await;
        match result {
            Err(AgentError::ExecutionFailed(message)) => {
                assert_eq!(message, "reserved failure");
            }
            other => panic!("expected reserved error failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reserved_return_writes_override_final_invoke_output() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ReturnAndOutputNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-reserved-return".to_string()),
            ..Default::default()
        };

        let output = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(output, json!({"kind": "final", "value": "hello"}));

        let state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(state.channels["out"], json!("hello"));
        assert_eq!(state.pending_writes.len(), 1);
        assert_eq!(
            state.pending_writes[0].1,
            crate::pregel::ReservedWrite::Return.as_str()
        );
    }

    #[tokio::test]
    async fn checkpoint_resume_writes_can_resume_next_invoke() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ResumeAwareNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-checkpoint-resume-write".to_string()),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let interrupted_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        let updated = runtime
            .update_state(
                config.clone(),
                StateUpdateRequest {
                    as_node: None,
                    values: json!({
                        crate::pregel::ReservedWrite::Resume.as_str(): {
                            "interrupt_id": interrupted_state.pending_interrupts[0].interrupt_id.clone(),
                            "value": "approved",
                        }
                    }),
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.pending_interrupts.len(), 1);
        assert_eq!(updated.pending_writes.len(), 1);

        let resumed = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(resumed["out"], json!("approved"));

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert!(final_state.pending_interrupts.is_empty());
        assert!(final_state.pending_writes.is_empty());
        assert_eq!(final_state.channels["out"], json!("approved"));
    }

    #[tokio::test]
    async fn interrupted_step_replays_successful_task_writes_without_rerun() {
        let runs = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("gate", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(CountingNode {
                runs: Arc::clone(&runs),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .add_node(Arc::new(SlowResumeInterruptNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["gate".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-replay-successful-writes".to_string()),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));
        assert_eq!(runs.load(Ordering::SeqCst), 1);

        let interrupted_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert!(interrupted_state
            .pending_writes
            .iter()
            .any(|(_, channel, value)| channel == "out" && value == "hello"));

        let resumed = runtime
            .invoke(
                json!({"in": "hello"}),
                Some(RunnableConfig {
                    resume_values_by_interrupt_id: [(
                        interrupted_state.pending_interrupts[0].interrupt_id.clone(),
                        json!("approved"),
                    )]
                    .into_iter()
                    .collect(),
                    ..config.clone()
                }),
            )
            .await
            .unwrap();
        assert_eq!(resumed["gate"], json!("approved"));
        assert_eq!(runs.load(Ordering::SeqCst), 1);

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(final_state.channels["out"], json!("hello"));
        assert_eq!(final_state.channels["gate"], json!("approved"));
    }

    #[tokio::test]
    async fn later_step_interrupt_replays_successful_pull_task_without_rerun() {
        let runs = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("mid", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("gate", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(RelayNode {
                name: "seed".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                input_key: "in".to_string(),
                output_key: "mid".to_string(),
            }))
            .add_node(Arc::new(CountingRelayNode {
                runs: Arc::clone(&runs),
                name: "counting_mid".to_string(),
                triggers: vec!["mid".to_string()],
                reads: vec!["mid".to_string()],
                input_key: "mid".to_string(),
                output_key: "out".to_string(),
            }))
            .add_node(Arc::new(SlowResumeInterruptNode {
                triggers: vec!["mid".to_string()],
                reads: vec!["mid".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["gate".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph)
            .with_checkpointer(checkpointer)
            .with_config(crate::pregel::PregelConfig {
                durability: crate::pregel::PregelDurability::Async,
                ..Default::default()
            });
        let config = RunnableConfig {
            thread_id: Some("thread-replay-later-step-pull".to_string()),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));
        assert_eq!(runs.load(Ordering::SeqCst), 1);

        let interrupted_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(interrupted_state.step, 1);
        assert!(interrupted_state
            .pending_writes
            .iter()
            .any(|(_, channel, value)| channel == "out" && value == "hello"));

        let resumed = runtime
            .invoke(
                json!({"in": "hello"}),
                Some(RunnableConfig {
                    resume_values_by_interrupt_id: [(
                        interrupted_state.pending_interrupts[0].interrupt_id.clone(),
                        json!("approved"),
                    )]
                    .into_iter()
                    .collect(),
                    ..config.clone()
                }),
            )
            .await
            .unwrap();
        assert_eq!(resumed["gate"], json!("approved"));
        assert_eq!(runs.load(Ordering::SeqCst), 1);

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(final_state.channels["out"], json!("hello"));
        assert_eq!(final_state.channels["gate"], json!("approved"));
    }

    #[tokio::test]
    async fn interrupted_step_replays_successful_push_task_without_rerun() {
        let worker_runs = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("mid", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("gate", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel(
                crate::pregel::TASKS_CHANNEL,
                ChannelSpec::new(ChannelKind::Tasks),
            )
            .add_node(Arc::new(RelayNode {
                name: "seed".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                input_key: "in".to_string(),
                output_key: "mid".to_string(),
            }))
            .add_node(Arc::new(MultiScheduledSendNode {
                triggers: vec!["mid".to_string()],
                reads: vec!["mid".to_string()],
            }))
            .add_node(Arc::new(CountingRelayNode {
                runs: Arc::clone(&worker_runs),
                name: "counting_push".to_string(),
                triggers: vec![crate::pregel::TASKS_CHANNEL.to_string()],
                reads: vec!["value".to_string()],
                input_key: "value".to_string(),
                output_key: "out".to_string(),
            }))
            .add_node(Arc::new(SlowResumeInterruptNode {
                triggers: vec![crate::pregel::TASKS_CHANNEL.to_string()],
                reads: vec!["value".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["gate".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph)
            .with_checkpointer(checkpointer)
            .with_config(crate::pregel::PregelConfig {
                durability: crate::pregel::PregelDurability::Async,
                ..Default::default()
            });
        let config = RunnableConfig {
            thread_id: Some("thread-replay-push-task".to_string()),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));
        assert_eq!(worker_runs.load(Ordering::SeqCst), 1);

        let interrupted_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(interrupted_state.step, 2);
        assert!(interrupted_state
            .pending_writes
            .iter()
            .any(|(_, channel, value)| channel == "out" && value == "hello"));

        let resumed = runtime
            .invoke(
                json!({"in": "hello"}),
                Some(RunnableConfig {
                    resume_values_by_interrupt_id: [(
                        interrupted_state.pending_interrupts[0].interrupt_id.clone(),
                        json!("approved"),
                    )]
                    .into_iter()
                    .collect(),
                    ..config.clone()
                }),
            )
            .await
            .unwrap();
        assert_eq!(resumed["gate"], json!("approved"));
        assert_eq!(worker_runs.load(Ordering::SeqCst), 1);

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(final_state.channels["out"], json!("hello"));
        assert_eq!(final_state.channels["gate"], json!("approved"));
    }

    #[tokio::test]
    async fn interrupted_step_preserves_all_successful_scheduled_writes_for_replay() {
        let sender_runs = Arc::new(AtomicUsize::new(0));
        let worker_a_runs = Arc::new(AtomicUsize::new(0));
        let worker_b_runs = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out_a", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out_b", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("gate", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel(
                crate::pregel::TASKS_CHANNEL,
                ChannelSpec::new(ChannelKind::Tasks),
            )
            .add_node(Arc::new(CountingMultiScheduledSendNode {
                runs: Arc::clone(&sender_runs),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .add_node(Arc::new(CountingRelayNode {
                runs: Arc::clone(&worker_a_runs),
                name: "worker_a".to_string(),
                triggers: vec![crate::pregel::TASKS_CHANNEL.to_string()],
                reads: vec!["value".to_string()],
                input_key: "value".to_string(),
                output_key: "out_a".to_string(),
            }))
            .add_node(Arc::new(CountingRelayNode {
                runs: Arc::clone(&worker_b_runs),
                name: "worker_b".to_string(),
                triggers: vec![crate::pregel::TASKS_CHANNEL.to_string()],
                reads: vec!["value".to_string()],
                input_key: "value".to_string(),
                output_key: "out_b".to_string(),
            }))
            .add_node(Arc::new(NamedResumeInterruptNode {
                name: "gatekeeper".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                interrupt_id: "gatekeeper-interrupt".to_string(),
                output_key: "gate".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec![
                "out_a".to_string(),
                "out_b".to_string(),
                "gate".to_string(),
            ])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-replay-multi-scheduled-writes".to_string()),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));
        assert_eq!(sender_runs.load(Ordering::SeqCst), 1);
        assert_eq!(worker_a_runs.load(Ordering::SeqCst), 0);
        assert_eq!(worker_b_runs.load(Ordering::SeqCst), 0);

        let interrupted_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(interrupted_state.pending_interrupts.len(), 1);
        assert_eq!(
            interrupted_state
                .pending_writes
                .iter()
                .filter(
                    |(_, channel, _)| channel == crate::pregel::ReservedWrite::Scheduled.as_str()
                )
                .count(),
            2
        );

        let resumed = runtime
            .invoke(
                json!({"in": "hello"}),
                Some(RunnableConfig {
                    resume_values_by_interrupt_id: [(
                        interrupted_state.pending_interrupts[0].interrupt_id.clone(),
                        json!("approved"),
                    )]
                    .into_iter()
                    .collect(),
                    ..config.clone()
                }),
            )
            .await
            .unwrap();
        assert_eq!(resumed["gate"], json!("approved"));
        assert_eq!(resumed["out_a"], json!("hello"));
        assert_eq!(resumed["out_b"], json!("hello"));
        assert_eq!(sender_runs.load(Ordering::SeqCst), 1);
        assert_eq!(worker_a_runs.load(Ordering::SeqCst), 1);
        assert_eq!(worker_b_runs.load(Ordering::SeqCst), 1);

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert!(final_state.pending_writes.is_empty());
        assert_eq!(final_state.channels["out_a"], json!("hello"));
        assert_eq!(final_state.channels["out_b"], json!("hello"));
        assert_eq!(final_state.channels["gate"], json!("approved"));
    }

    #[tokio::test]
    async fn resume_by_interrupt_id_preserves_unmatched_pending_interrupts() {
        let checkpointer = Arc::new(MemorySaver::new());

        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ResumeAwareNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer.clone());
        let config = RunnableConfig {
            thread_id: Some("thread-resume-selective-id".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let interrupted_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        let mut checkpoint = checkpointer.get_tuple(&config).await.unwrap().unwrap().0;
        checkpoint.pending_interrupts.push(
            serde_json::to_value(crate::pregel::InterruptRecord {
                interrupt_id: "other-interrupt".to_string(),
                namespace: "other".to_string(),
                task_id: "other-task".to_string(),
                node_name: "other-node".to_string(),
                step: 0,
                value: json!({"kind": "approval"}),
            })
            .unwrap(),
        );
        checkpointer.put(&config, &checkpoint).await.unwrap();

        let resumed = runtime
            .invoke(
                json!({"in": "hello"}),
                Some(RunnableConfig {
                    resume_values_by_interrupt_id: [(
                        interrupted_state.pending_interrupts[0].interrupt_id.clone(),
                        json!("approved"),
                    )]
                    .into_iter()
                    .collect(),
                    ..config.clone()
                }),
            )
            .await
            .unwrap();
        assert_eq!(resumed["out"], json!("approved"));

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(final_state.pending_interrupts.len(), 1);
        assert_eq!(
            final_state.pending_interrupts[0].interrupt_id,
            "other-interrupt"
        );
    }

    #[tokio::test]
    async fn resume_by_namespace_only_consumes_matching_namespace() {
        let checkpointer = Arc::new(MemorySaver::new());

        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ResumeAwareNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer.clone());
        let config = RunnableConfig {
            thread_id: Some("thread-resume-selective-namespace".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let mut checkpoint = checkpointer.get_tuple(&config).await.unwrap().unwrap().0;
        checkpoint.pending_interrupts.push(
            serde_json::to_value(crate::pregel::InterruptRecord {
                interrupt_id: "other-ns-interrupt".to_string(),
                namespace: "other".to_string(),
                task_id: "other-task".to_string(),
                node_name: "other-node".to_string(),
                step: 0,
                value: json!({"kind": "approval"}),
            })
            .unwrap(),
        );
        checkpointer.put(&config, &checkpoint).await.unwrap();

        let resumed = runtime
            .invoke(
                json!({"in": "hello"}),
                Some(RunnableConfig {
                    resume_values_by_namespace: [("main".to_string(), json!("approved"))]
                        .into_iter()
                        .collect(),
                    ..config.clone()
                }),
            )
            .await
            .unwrap();
        assert_eq!(resumed["out"], json!("approved"));

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(final_state.pending_interrupts.len(), 1);
        assert_eq!(final_state.pending_interrupts[0].namespace, "other");
    }

    #[tokio::test]
    async fn generic_resume_value_does_not_ambiguously_consume_multiple_interrupts() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out_a", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out_b", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(NamedResumeInterruptNode {
                name: "resume_a".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                interrupt_id: "interrupt-a".to_string(),
                output_key: "out_a".to_string(),
            }))
            .add_node(Arc::new(NamedResumeInterruptNode {
                name: "resume_b".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                interrupt_id: "interrupt-b".to_string(),
                output_key: "out_b".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out_a".to_string(), "out_b".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-generic-resume-ambiguous".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let second = runtime
            .invoke(
                json!({"in": "hello"}),
                Some(RunnableConfig {
                    resume_value: Some(json!("approved")),
                    ..config.clone()
                }),
            )
            .await;
        assert!(matches!(second, Err(AgentError::Interrupted(_))));

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(final_state.pending_interrupts.len(), 2);
        assert!(final_state.channels["out_a"].is_null());
        assert!(final_state.channels["out_b"].is_null());
    }

    #[tokio::test]
    async fn generic_resume_write_does_not_ambiguously_consume_multiple_interrupts() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out_a", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out_b", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(NamedResumeInterruptNode {
                name: "resume_a".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                interrupt_id: "interrupt-a".to_string(),
                output_key: "out_a".to_string(),
            }))
            .add_node(Arc::new(NamedResumeInterruptNode {
                name: "resume_b".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                interrupt_id: "interrupt-b".to_string(),
                output_key: "out_b".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out_a".to_string(), "out_b".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-generic-resume-write-ambiguous".to_string()),
            checkpoint_ns: "main".to_string(),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let updated = runtime
            .update_state(
                config.clone(),
                StateUpdateRequest {
                    as_node: None,
                    values: json!({
                        crate::pregel::ReservedWrite::Resume.as_str(): "approved"
                    }),
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.pending_interrupts.len(), 2);

        let second = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        assert!(second["out_a"].is_null());
        assert!(second["out_b"].is_null());

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(final_state.pending_interrupts.len(), 2);
        assert!(final_state.channels["out_a"].is_null());
        assert!(final_state.channels["out_b"].is_null());
    }

    #[tokio::test]
    async fn reserved_scheduled_writes_enqueue_followup_tasks() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel(
                crate::pregel::TASKS_CHANNEL,
                ChannelSpec::new(ChannelKind::Tasks),
            )
            .add_node(Arc::new(ScheduledSendNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .add_node(Arc::new(RelayNode {
                name: "worker".to_string(),
                triggers: vec![crate::pregel::TASKS_CHANNEL.to_string()],
                reads: vec!["value".to_string()],
                input_key: "value".to_string(),
                output_key: "out".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(graph);
        let output = runtime.invoke(json!({"in": "hello"}), None).await.unwrap();
        assert_eq!(output["out"], json!("hello"));
    }

    #[tokio::test]
    async fn no_writes_marker_is_successful_without_pending_write_pollution() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(NoWritesMarkerNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-no-writes-marker".to_string()),
            ..Default::default()
        };

        runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();

        let state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(state.step, 1);
        assert!(state.pending_writes.is_empty());
    }

    #[tokio::test]
    async fn managed_values_are_injected_into_node_input() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ManagedValueNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(graph).with_managed_value("tenant", json!("acme"));
        let output = runtime.invoke(json!({"in": "hello"}), None).await.unwrap();
        assert_eq!(output["out"], json!("acme"));
    }

    #[tokio::test]
    async fn resume_map_rehydrates_interrupting_task() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ResumeAwareNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-resume".to_string()),
            ..Default::default()
        };

        let first = runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let interrupted_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(interrupted_state.pending_interrupts.len(), 1);

        let resumed = runtime
            .invoke(
                json!({"in": "hello"}),
                Some(RunnableConfig {
                    resume_values_by_interrupt_id: [(
                        interrupted_state.pending_interrupts[0].interrupt_id.clone(),
                        json!("approved"),
                    )]
                    .into_iter()
                    .collect(),
                    ..config.clone()
                }),
            )
            .await
            .unwrap();
        assert_eq!(resumed["out"], json!("approved"));

        let final_state = runtime.get_state(config).await.unwrap().unwrap();
        assert!(final_state.pending_interrupts.is_empty());
        assert_eq!(final_state.channels["out"], json!("approved"));
    }

    #[tokio::test]
    async fn invoke_subgraph_uses_isolated_checkpoint_namespace() {
        let checkpointer = Arc::new(MemorySaver::new());

        let mut parent_graph = PregelGraph::new();
        parent_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();
        let parent_runtime =
            PregelRuntime::new(parent_graph).with_checkpointer(checkpointer.clone());

        let mut child_graph = PregelGraph::new();
        child_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();
        let child_runtime = PregelRuntime::new(child_graph).with_checkpointer(checkpointer);

        let base_config = RunnableConfig {
            thread_id: Some("thread-subgraph".to_string()),
            checkpoint_ns: "parent".to_string(),
            ..Default::default()
        };

        let parent_output = parent_runtime
            .invoke(json!({"in": "parent"}), Some(base_config.clone()))
            .await
            .unwrap();
        assert_eq!(parent_output["out"], json!("parent"));

        let result = parent_runtime
            .invoke_subgraph(
                &child_runtime,
                base_config.clone(),
                SubgraphInvocation {
                    parent_task_id: "parent-task".to_string(),
                    parent_checkpoint_id: None,
                    child_namespace: CheckpointNamespace("parent/child-a".to_string()),
                    entry_input: json!({"in": "child"}),
                },
            )
            .await
            .unwrap();

        match result {
            SubgraphResult::Completed(value) => assert_eq!(value["out"], json!("child")),
            other => panic!("expected completed subgraph, got {other:?}"),
        }

        let parent_history = parent_runtime
            .get_state_history(base_config.clone(), None, None, None)
            .await
            .unwrap();
        assert_eq!(parent_history.len(), 1);

        let child_history = child_runtime
            .get_state_history(
                RunnableConfig {
                    checkpoint_ns: "parent/child-a".to_string(),
                    ..base_config
                },
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(child_history.len(), 1);
    }

    #[tokio::test]
    async fn invoke_subgraph_can_resume_interrupting_child_namespace() {
        let checkpointer = Arc::new(MemorySaver::new());

        let mut parent_graph = PregelGraph::new();
        parent_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();
        let parent_runtime =
            PregelRuntime::new(parent_graph).with_checkpointer(checkpointer.clone());

        let mut child_graph = PregelGraph::new();
        child_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ResumeAwareNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();
        let child_runtime = PregelRuntime::new(child_graph).with_checkpointer(checkpointer);

        let base_config = RunnableConfig {
            thread_id: Some("thread-subgraph-resume".to_string()),
            checkpoint_ns: "parent".to_string(),
            ..Default::default()
        };
        let invocation = SubgraphInvocation {
            parent_task_id: "parent-task".to_string(),
            parent_checkpoint_id: None,
            child_namespace: CheckpointNamespace("parent/child-resume".to_string()),
            entry_input: json!({"in": "child"}),
        };

        let first = parent_runtime
            .invoke_subgraph(&child_runtime, base_config.clone(), invocation.clone())
            .await
            .unwrap();
        let interrupt_id = match first {
            SubgraphResult::Interrupted(record) => {
                assert_eq!(record.namespace, "parent/child-resume");
                record.interrupt_id
            }
            other => panic!("expected interrupted subgraph, got {other:?}"),
        };

        let resumed = parent_runtime
            .invoke_subgraph(
                &child_runtime,
                RunnableConfig {
                    resume_values_by_interrupt_id: [(interrupt_id, json!("approved"))]
                        .into_iter()
                        .collect(),
                    ..base_config.clone()
                },
                invocation,
            )
            .await
            .unwrap();

        match resumed {
            SubgraphResult::Completed(value) => assert_eq!(value["out"], json!("approved")),
            other => panic!("expected completed resumed subgraph, got {other:?}"),
        }

        let child_history = child_runtime
            .get_state_history(
                RunnableConfig {
                    checkpoint_ns: "parent/child-resume".to_string(),
                    ..base_config
                },
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(child_history.len(), 2);
    }

    #[tokio::test]
    async fn invoke_subgraph_supports_nested_checkpoint_namespaces() {
        let checkpointer = Arc::new(MemorySaver::new());

        let mut root_graph = PregelGraph::new();
        root_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();
        let root_runtime = PregelRuntime::new(root_graph).with_checkpointer(checkpointer.clone());

        let mut child_graph = PregelGraph::new();
        child_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();
        let child_runtime = PregelRuntime::new(child_graph).with_checkpointer(checkpointer.clone());

        let mut grandchild_graph = PregelGraph::new();
        grandchild_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();
        let grandchild_runtime =
            PregelRuntime::new(grandchild_graph).with_checkpointer(checkpointer);

        let base_config = RunnableConfig {
            thread_id: Some("thread-nested-subgraph".to_string()),
            checkpoint_ns: "root".to_string(),
            ..Default::default()
        };

        let child_result = root_runtime
            .invoke_subgraph(
                &child_runtime,
                base_config.clone(),
                SubgraphInvocation {
                    parent_task_id: "root-task".to_string(),
                    parent_checkpoint_id: None,
                    child_namespace: CheckpointNamespace("root/child".to_string()),
                    entry_input: json!({"in": "child"}),
                },
            )
            .await
            .unwrap();
        match child_result {
            SubgraphResult::Completed(value) => assert_eq!(value["out"], json!("child")),
            other => panic!("expected completed child subgraph, got {other:?}"),
        }

        let grandchild_result = child_runtime
            .invoke_subgraph(
                &grandchild_runtime,
                RunnableConfig {
                    checkpoint_ns: "root/child".to_string(),
                    ..base_config.clone()
                },
                SubgraphInvocation {
                    parent_task_id: "child-task".to_string(),
                    parent_checkpoint_id: None,
                    child_namespace: CheckpointNamespace("root/child/grandchild".to_string()),
                    entry_input: json!({"in": "grandchild"}),
                },
            )
            .await
            .unwrap();
        match grandchild_result {
            SubgraphResult::Completed(value) => assert_eq!(value["out"], json!("grandchild")),
            other => panic!("expected completed grandchild subgraph, got {other:?}"),
        }

        let child_history = child_runtime
            .get_state_history(
                RunnableConfig {
                    checkpoint_ns: "root/child".to_string(),
                    ..base_config.clone()
                },
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(child_history.len(), 1);

        let grandchild_history = grandchild_runtime
            .get_state_history(
                RunnableConfig {
                    checkpoint_ns: "root/child/grandchild".to_string(),
                    ..base_config
                },
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(grandchild_history.len(), 1);
    }

    #[derive(Debug)]
    struct FailThenSucceedNode {
        attempts: Arc<AtomicUsize>,
        fail_until: usize,
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[async_trait]
    impl PregelNode for FailThenSucceedNode {
        fn name(&self) -> &str {
            "flaky"
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            if attempt < self.fail_until {
                return Err(AgentError::ExecutionFailed(format!(
                    "transient error #{attempt}"
                )));
            }
            let value = input.read_values.get("in").cloned().unwrap_or(json!(null));
            Ok(PregelNodeOutput {
                writes: vec![("out".to_string(), value)],
            })
        }
    }

    #[tokio::test]
    async fn retry_policy_retries_before_failing() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(FailThenSucceedNode {
                attempts: Arc::clone(&attempts),
                fail_until: 2,
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(graph).with_config(crate::pregel::PregelConfig {
            retry_policy: crate::graph::RetryPolicy::fixed(3, std::time::Duration::ZERO),
            ..Default::default()
        });
        let output = runtime.invoke(json!({"in": "ok"}), None).await.unwrap();
        assert_eq!(output["out"], json!("ok"));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_policy_exhaustion_propagates_error() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(FailThenSucceedNode {
                attempts: Arc::clone(&attempts),
                fail_until: 10,
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(graph).with_config(crate::pregel::PregelConfig {
            retry_policy: crate::graph::RetryPolicy::fixed(2, std::time::Duration::ZERO),
            ..Default::default()
        });
        let result = runtime.invoke(json!({"in": "ok"}), None).await;
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[derive(Debug)]
    struct AppendNode {
        name: String,
        triggers: Vec<String>,
        reads: Vec<String>,
        read_key: String,
        write_channel: String,
        suffix: String,
    }

    #[async_trait]
    impl PregelNode for AppendNode {
        fn name(&self) -> &str {
            &self.name
        }

        fn triggers(&self) -> &[String] {
            &self.triggers
        }

        fn reads(&self) -> &[String] {
            &self.reads
        }

        async fn run(
            &self,
            input: PregelNodeInput,
            _ctx: &crate::pregel::node::PregelNodeContext,
        ) -> Result<PregelNodeOutput, AgentError> {
            let base = input
                .read_values
                .get(&self.read_key)
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let result = format!("{}{}", base, self.suffix);
            Ok(PregelNodeOutput {
                writes: vec![(self.write_channel.clone(), json!(result))],
            })
        }
    }

    #[tokio::test]
    async fn multi_step_pipeline_with_versions_seen() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("mid", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(AppendNode {
                name: "step1".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                read_key: "in".to_string(),
                write_channel: "mid".to_string(),
                suffix: "+A".to_string(),
            }))
            .add_node(Arc::new(AppendNode {
                name: "step2".to_string(),
                triggers: vec!["mid".to_string()],
                reads: vec!["mid".to_string()],
                read_key: "mid".to_string(),
                write_channel: "out".to_string(),
                suffix: "+B".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
        let config = RunnableConfig {
            thread_id: Some("thread-pipeline".to_string()),
            ..Default::default()
        };

        let output = runtime
            .invoke(json!({"in": "x"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(output["in"], json!("x"));
        assert_eq!(output["mid"], json!("x+A"));
        assert_eq!(output["out"], json!("x+A+B"));

        let state = runtime.get_state(config).await.unwrap().unwrap();
        assert_eq!(state.step, 2);
        assert_eq!(state.channels["out"], json!("x+A+B"));
    }

    #[tokio::test]
    async fn topic_channel_consume_clears_between_steps() {
        use crate::pregel::channel::{Channel, TopicChannel};

        let mut ch = TopicChannel::new(false);
        ch.update(&[json!("a"), json!("b")]);
        assert_eq!(ch.snapshot(), json!(["a", "b"]));

        assert!(ch.consume());
        assert_eq!(ch.snapshot(), json!([]));

        ch.update(&[json!("c")]);
        assert_eq!(ch.snapshot(), json!(["c"]));
    }

    #[tokio::test]
    async fn task_cache_does_not_store_reserved_control_writes() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(ReservedInterruptNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let cache = Arc::new(InMemoryPregelTaskCache::new());
        let runtime = PregelRuntime::new(graph).with_task_cache(cache.clone());

        let result = runtime.invoke(json!({"in": "hello"}), None).await;
        assert!(matches!(result, Err(AgentError::Interrupted(_))));

        for (_key, entry) in cache.entries() {
            for (ch, _) in &entry.writes {
                assert!(
                    ch != "__interrupt__" && ch != "__error__" && ch != "__return__",
                    "reserved control write {ch} must not be stored in task cache"
                );
            }
        }
    }

    #[tokio::test]
    async fn task_cache_is_isolated_by_thread_id() {
        let runs = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(CountingNode {
                runs: Arc::clone(&runs),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let cache = Arc::new(InMemoryPregelTaskCache::new());
        let runtime = PregelRuntime::new(graph).with_task_cache(cache.clone());

        let config_a = RunnableConfig {
            thread_id: Some("thread-a".to_string()),
            ..Default::default()
        };
        let config_b = RunnableConfig {
            thread_id: Some("thread-b".to_string()),
            ..Default::default()
        };

        runtime
            .invoke(json!({"in": "hello"}), Some(config_a.clone()))
            .await
            .unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 1, "first invoke runs the node");

        runtime
            .invoke(json!({"in": "hello"}), Some(config_b.clone()))
            .await
            .unwrap();
        assert_eq!(
            runs.load(Ordering::SeqCst),
            2,
            "different thread_id must not reuse cache"
        );

        runtime
            .invoke(json!({"in": "hello"}), Some(config_a))
            .await
            .unwrap();
        assert_eq!(
            runs.load(Ordering::SeqCst),
            2,
            "same thread_id should reuse cache"
        );
    }

    #[tokio::test]
    async fn ephemeral_channel_clears_between_steps() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("tmp", ChannelSpec::new(ChannelKind::Ephemeral))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(RelayNode {
                name: "writer".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                input_key: "in".to_string(),
                output_key: "tmp".to_string(),
            }))
            .add_node(Arc::new(RelayNode {
                name: "reader".to_string(),
                triggers: vec!["tmp".to_string()],
                reads: vec!["tmp".to_string()],
                input_key: "tmp".to_string(),
                output_key: "out".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(graph);
        let result = runtime
            .invoke(json!({"in": "ephemeral"}), None)
            .await
            .unwrap();
        assert_eq!(result["out"], json!("ephemeral"));
    }

    #[tokio::test]
    async fn named_barrier_channel_gates_downstream_until_all_names_written() {
        use crate::pregel::channel::{Channel as _, NamedBarrierChannel};

        let mut ch = NamedBarrierChannel::new(["step_a".to_string(), "step_b".to_string()]);
        assert_eq!(ch.snapshot(), serde_json::Value::Null);
        assert!(!ch.consume());

        ch.update(&[json!("step_a")]);
        assert_eq!(ch.snapshot(), serde_json::Value::Null);
        assert!(!ch.consume());

        ch.update(&[json!("step_b")]);
        assert_eq!(ch.snapshot(), json!(true));
        assert!(ch.consume());
        assert_eq!(ch.snapshot(), serde_json::Value::Null);
    }

    #[tokio::test]
    async fn versions_seen_scoped_to_node_channels() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("a_out", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("b_out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(RelayNode {
                name: "node_a".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                input_key: "in".to_string(),
                output_key: "a_out".to_string(),
            }))
            .add_node(Arc::new(RelayNode {
                name: "node_b".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                input_key: "in".to_string(),
                output_key: "b_out".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["a_out".to_string(), "b_out".to_string()])
            .build_trigger_index();

        let checkpointer = Arc::new(MemorySaver::new());
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer.clone());
        let config = RunnableConfig {
            thread_id: Some("thread-versions-seen".to_string()),
            ..Default::default()
        };

        runtime
            .invoke(json!({"in": "x"}), Some(config.clone()))
            .await
            .unwrap();

        let (checkpoint, _) = checkpointer.get_tuple(&config).await.unwrap().unwrap();

        let a_seen = &checkpoint.versions_seen["node_a"];
        assert!(
            !a_seen.contains_key("b_out"),
            "node_a does not read or trigger on b_out, so it must not appear in its versions_seen"
        );

        let b_seen = &checkpoint.versions_seen["node_b"];
        assert!(
            !b_seen.contains_key("a_out"),
            "node_b does not read or trigger on a_out, so it must not appear in its versions_seen"
        );
    }

    #[tokio::test]
    async fn get_graph_returns_static_graph_view() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("memory", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(RelayNode {
                name: "worker".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string(), "memory".to_string()],
                input_key: "in".to_string(),
                output_key: "out".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(graph);
        let view = runtime.get_graph().expect("graph view");

        assert_eq!(view.nodes.len(), 1);
        assert_eq!(view.channels.len(), 3);
        assert_eq!(view.edges.len(), 2);
        assert!(view.to_mermaid().contains("flowchart TD"));
    }

    #[tokio::test]
    async fn get_subgraphs_discovers_inline_child_runtime() {
        let mut child_graph = PregelGraph::new();
        child_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(EchoNode {
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();
        let child_runtime = Arc::new(PregelRuntime::new(child_graph));

        let mut parent_graph = PregelGraph::new();
        parent_graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(InlineSubgraphNode {
                name: "parent".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                child_runtime,
                base_namespace: CheckpointNamespace("child".to_string()),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let runtime = PregelRuntime::new(parent_graph);
        let subgraphs = runtime.get_subgraphs(true).expect("subgraphs");

        assert_eq!(subgraphs.len(), 1);
        assert_eq!(subgraphs[0].path, "parent/child");
        assert_eq!(
            subgraphs[0].runtime.get_graph().expect("child graph").nodes[0].name,
            "echo"
        );
        assert_eq!(
            runtime
                .get_graph_xray(true)
                .expect("recursive graph")
                .subgraphs[0]
                .path,
            "parent/child"
        );
    }

    #[tokio::test]
    async fn clear_cache_forces_task_recomputation() {
        let runs = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(CountingNode {
                runs: Arc::clone(&runs),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["out".to_string()])
            .build_trigger_index();

        let cache = Arc::new(InMemoryPregelTaskCache::new());
        let runtime = PregelRuntime::new(graph).with_task_cache(cache.clone());
        let config = RunnableConfig {
            thread_id: Some("thread-clear-cache".to_string()),
            ..Default::default()
        };

        runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(
            runs.load(Ordering::SeqCst),
            1,
            "second invoke should hit cache"
        );

        runtime.clear_cache().expect("cache clear should succeed");

        runtime
            .invoke(json!({"in": "hello"}), Some(config))
            .await
            .unwrap();
        assert_eq!(
            runs.load(Ordering::SeqCst),
            2,
            "invoke after clear_cache should recompute the task"
        );
    }

    #[tokio::test]
    async fn clear_cache_for_nodes_keeps_other_cached_nodes() {
        let node_a_runs = Arc::new(AtomicUsize::new(0));
        let node_b_runs = Arc::new(AtomicUsize::new(0));
        let mut graph = PregelGraph::new();
        graph
            .add_channel("in", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("a_out", ChannelSpec::new(ChannelKind::LastValue))
            .add_channel("b_out", ChannelSpec::new(ChannelKind::LastValue))
            .add_node(Arc::new(CountingRelayNode {
                runs: Arc::clone(&node_a_runs),
                name: "node_a".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                input_key: "in".to_string(),
                output_key: "a_out".to_string(),
            }))
            .add_node(Arc::new(CountingRelayNode {
                runs: Arc::clone(&node_b_runs),
                name: "node_b".to_string(),
                triggers: vec!["in".to_string()],
                reads: vec!["in".to_string()],
                input_key: "in".to_string(),
                output_key: "b_out".to_string(),
            }))
            .set_input_channels(vec!["in".to_string()])
            .set_output_channels(vec!["a_out".to_string(), "b_out".to_string()])
            .build_trigger_index();

        let cache = Arc::new(InMemoryPregelTaskCache::new());
        let runtime = PregelRuntime::new(graph).with_task_cache(cache);
        let config = RunnableConfig {
            thread_id: Some("thread-clear-cache-for-nodes".to_string()),
            ..Default::default()
        };

        runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        runtime
            .invoke(json!({"in": "hello"}), Some(config.clone()))
            .await
            .unwrap();
        assert_eq!(node_a_runs.load(Ordering::SeqCst), 1);
        assert_eq!(node_b_runs.load(Ordering::SeqCst), 1);

        runtime
            .clear_cache_for_nodes(&["node_a".to_string()])
            .expect("selective cache clear");

        runtime
            .invoke(json!({"in": "hello"}), Some(config))
            .await
            .unwrap();
        assert_eq!(node_a_runs.load(Ordering::SeqCst), 2);
        assert_eq!(node_b_runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn extract_summary_from_channel_values_returns_summary_when_present() {
        let channel_values = serde_json::json!({"summary": "Hello world", "messages": []});
        assert_eq!(
            extract_summary_from_channel_values(&channel_values),
            Some("Hello world".to_string())
        );
    }

    #[test]
    fn extract_summary_from_channel_values_returns_none_when_null() {
        let channel_values = serde_json::json!({"summary": null, "messages": []});
        assert_eq!(extract_summary_from_channel_values(&channel_values), None);
    }

    #[test]
    fn extract_summary_from_channel_values_returns_none_when_empty_string() {
        let channel_values = serde_json::json!({"summary": "", "messages": []});
        assert_eq!(extract_summary_from_channel_values(&channel_values), None);
    }

    #[test]
    fn extract_summary_from_channel_values_returns_none_when_field_missing() {
        let channel_values = serde_json::json!({"messages": []});
        assert_eq!(extract_summary_from_channel_values(&channel_values), None);
    }
}
