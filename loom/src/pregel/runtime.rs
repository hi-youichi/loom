//! Public Pregel runtime entrypoints.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;

use crate::error::AgentError;
use crate::memory::{
    Checkpoint, CheckpointError, CheckpointListItem, CheckpointSource, Checkpointer,
    RunnableConfig, Store,
};
use crate::pregel::algo::{restore_channels_from_checkpoint, task_cache_key};
use crate::pregel::cache::{CachedTaskWrites, PregelTaskCache};
use crate::pregel::config::PregelConfig;
use crate::pregel::loop_state::PregelLoop;
use crate::pregel::node::{PregelGraph, PregelNodeContext};
use crate::pregel::replay::{ReplayMode, ReplayRequest, ReplayResult};
use crate::pregel::runner::PregelRunner;
use crate::pregel::state::{BulkStateUpdateRequest, PregelStateSnapshot, StateUpdateRequest};
use crate::pregel::subgraph::{SubgraphInvocation, SubgraphResult};
use crate::pregel::types::{ChannelValue, ManagedValues, ResumeMap};
use crate::stream::{StreamEvent, StreamMode};

/// Stream handle for a Pregel run.
pub struct PregelStream {
    pub events: ReceiverStream<StreamEvent<ChannelValue>>,
    pub completion: JoinHandle<Result<ChannelValue, AgentError>>,
}

struct PendingCheckpointWrite {
    checkpoint: Checkpoint<ChannelValue>,
    completion: JoinHandle<Result<(), AgentError>>,
}

/// Public runtime entrypoint for Pregel graph execution.
#[derive(Clone)]
pub struct PregelRuntime {
    graph: Arc<PregelGraph>,
    checkpointer: Option<Arc<dyn Checkpointer<ChannelValue>>>,
    task_cache: Option<Arc<dyn PregelTaskCache>>,
    managed_values: ManagedValues,
    store: Option<Arc<dyn Store>>,
    config: PregelConfig,
}

impl std::fmt::Debug for PregelRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PregelRuntime")
            .field("graph", &self.graph)
            .field("has_checkpointer", &self.checkpointer.is_some())
            .field("has_task_cache", &self.task_cache.is_some())
            .field("has_store", &self.store.is_some())
            .field("config", &self.config)
            .finish()
    }
}

impl PregelRuntime {
    /// Creates a new runtime for a graph definition.
    pub fn new(graph: PregelGraph) -> Self {
        Self {
            graph: Arc::new(graph),
            checkpointer: None,
            task_cache: None,
            managed_values: ManagedValues::default(),
            store: None,
            config: PregelConfig::default(),
        }
    }

    /// Attaches a checkpointer to the runtime.
    pub fn with_checkpointer(self, checkpointer: Arc<dyn Checkpointer<ChannelValue>>) -> Self {
        Self {
            checkpointer: Some(checkpointer),
            ..self
        }
    }

    /// Attaches a long-term store to the runtime.
    pub fn with_store(self, store: Arc<dyn Store>) -> Self {
        Self {
            store: Some(store),
            ..self
        }
    }

    /// Attaches a task cache for cached-write reuse.
    pub fn with_task_cache(self, task_cache: Arc<dyn PregelTaskCache>) -> Self {
        Self {
            task_cache: Some(task_cache),
            ..self
        }
    }

    /// Replaces managed runtime values injected into each task.
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
    pub fn with_config(self, config: PregelConfig) -> Self {
        Self { config, ..self }
    }

    /// Returns the graph definition.
    pub fn graph(&self) -> &Arc<PregelGraph> {
        &self.graph
    }

    /// Initializes loop state for a run.
    pub async fn init_loop(
        &self,
        input: ChannelValue,
        config: Option<RunnableConfig>,
    ) -> Result<PregelLoop, AgentError> {
        let config = config.unwrap_or_default();
        validate_checkpointer_config(self.checkpointer.as_ref(), &config)?;
        let checkpoint = match &self.checkpointer {
            Some(checkpointer) => match checkpointer.get_tuple(&config).await {
                Ok(Some((checkpoint, _metadata))) => checkpoint,
                Ok(None) => Checkpoint::from_state(input, CheckpointSource::Input, 0),
                Err(error) => return Err(checkpoint_error(error)),
            },
            None => Checkpoint::from_state(input, CheckpointSource::Input, 0),
        };
        let channels = restore_channels_from_checkpoint(&checkpoint, &self.graph);
        let mut loop_state = PregelLoop::new(
            Arc::clone(&self.graph),
            config.checkpoint_ns.clone(),
            checkpoint,
            channels,
            self.config.clone(),
        );
        loop_state.interrupts.pending_resume_values = resume_map_from_config(&config)
            .values_by_interrupt_id
            .values()
            .cloned()
            .chain(
                resume_map_from_config(&config)
                    .values_by_namespace
                    .values()
                    .cloned(),
            )
            .collect();
        Ok(loop_state)
    }

    /// Invokes the runtime once. Current implementation is a skeleton.
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
        let resume_map = resume_map_from_config(&run_config);
        let node_ctx = PregelNodeContext {
            cancellation: None,
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
                let tasks = self.attach_cached_writes(tasks);
                let outcomes = runner
                    .run_step(tasks, Arc::clone(&loop_state.graph), node_ctx.clone())
                    .await;
                self.store_successful_task_writes(&outcomes);
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
                        flush_inflight_checkpoint(
                            &mut inflight_checkpoint,
                            &node_ctx,
                            &run_config,
                        )
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
                    flush_inflight_checkpoint(&mut inflight_checkpoint, &node_ctx, &run_config).await?;
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

        if let Err(AgentError::Interrupted(interrupt)) = &result {
            flush_inflight_checkpoint(&mut inflight_checkpoint, &node_ctx, &run_config).await?;
            self.persist_checkpoint(
                &mut loop_state,
                &run_config,
                &node_ctx,
                CheckpointSource::Loop,
            )
            .await?;
            return Err(AgentError::Interrupted(interrupt.clone()));
        }

        result?;
        Ok(loop_state.output())
    }

    /// Starts a streamed run. Current implementation emits no intermediate events.
    pub fn stream(&self, input: ChannelValue, config: Option<RunnableConfig>) -> PregelStream {
        let (tx, rx) = mpsc::channel(64);
        let runtime = self.clone();
        let completion = tokio::spawn(async move {
            runtime.invoke_inner(input, config, Some(tx)).await
        });
        PregelStream {
            events: ReceiverStream::new(rx),
            completion,
        }
    }

    /// Loads the latest checkpoint-backed runtime state.
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
        Ok(checkpoint
            .as_ref()
            .map(PregelStateSnapshot::from_checkpoint))
    }

    /// Lists checkpoint history metadata for a run.
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
    pub async fn bulk_update_state(
        &self,
        config: RunnableConfig,
        request: BulkStateUpdateRequest,
    ) -> Result<PregelStateSnapshot, AgentError> {
        validate_checkpointer_config(self.checkpointer.as_ref(), &config)?;
        let mut checkpoint = self.load_checkpoint_or_default(&config, serde_json::json!({})).await?;
        let mut channels = restore_channels_from_checkpoint(&checkpoint, &self.graph);
        let tasks = request
            .updates
            .iter()
            .enumerate()
            .map(|(index, update)| synthetic_update_task(index, checkpoint.metadata.step.max(0) as u64, update, &self.graph))
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
                let Some((checkpoint, _metadata)) = checkpointer
                    .get_tuple(&source_config)
                    .await
                    .map_err(checkpoint_error)? else {
                    return Ok(None);
                };

                let mut forked = checkpoint.copy();
                forked.id = crate::memory::uuid6().to_string();
                forked.metadata.source = CheckpointSource::Fork;
                forked
                    .metadata
                    .parents
                    .insert(source_config.checkpoint_ns.clone(), checkpoint_id);
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
        let child_namespace = invocation.child_namespace.0.clone();
        let child_config = RunnableConfig {
            checkpoint_ns: child_namespace.clone(),
            checkpoint_id: None,
            depth: Some(config.depth.unwrap_or(0) + 1),
            ..config
        };
        match child_runtime
            .invoke_inner(invocation.entry_input, Some(child_config.clone()), stream_tx)
            .await
        {
            Ok(value) => Ok(SubgraphResult::Completed(value)),
            Err(AgentError::Interrupted(interrupt)) => {
                if let Some(state) = child_runtime.get_state(child_config.clone()).await? {
                    if let Some(mut record) = state.pending_interrupts.into_iter().next() {
                        if record.namespace.is_empty() {
                            record.namespace = child_namespace.clone();
                        }
                        return Ok(SubgraphResult::Interrupted(record));
                    }
                }

                Ok(SubgraphResult::Interrupted(crate::pregel::InterruptRecord {
                    interrupt_id: interrupt
                        .0
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("subgraph:{}", invocation.parent_task_id)),
                    namespace: child_namespace,
                    task_id: invocation.parent_task_id,
                    node_name: "subgraph".to_string(),
                    step: 0,
                    value: interrupt.0.value,
                }))
            }
            Err(AgentError::Cancelled) => Ok(SubgraphResult::Cancelled),
            Err(error) => Ok(SubgraphResult::Failed(error.to_string())),
        }
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
                Ok(Some((checkpoint, _metadata))) => Ok(checkpoint),
                Ok(None) => Ok(Checkpoint::from_state(fallback_input, CheckpointSource::Input, 0)),
                Err(error) => Err(checkpoint_error(error)),
            },
            None => Ok(Checkpoint::from_state(fallback_input, CheckpointSource::Input, 0)),
        }
    }

    fn attach_cached_writes(
        &self,
        tasks: Vec<crate::pregel::PreparedTask>,
    ) -> Vec<crate::pregel::PreparedTask> {
        let Some(cache) = &self.task_cache else {
            return tasks;
        };
        tasks
            .into_iter()
            .map(|mut task| {
                if task.cached_writes.is_empty() {
                    if let Some(cached) = cache.get(&task_cache_key(&task)) {
                        task.cached_writes = cached.writes;
                    }
                }
                task
            })
            .collect()
    }

    fn store_successful_task_writes(
        &self,
        outcomes: &[crate::pregel::TaskOutcome],
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
            cache.put(
                task_cache_key(&task.prepared),
                CachedTaskWrites {
                    task_id: task.prepared.id.clone(),
                    writes: task.writes.clone(),
                },
            );
        }
    }
}

async fn emit_values_event(
    ctx: &PregelNodeContext,
    state: &ChannelValue,
) {
    if !(ctx.stream_mode.contains(&StreamMode::Values) || ctx.stream_mode.contains(&StreamMode::Debug))
    {
        return;
    }
    if let Some(tx) = &ctx.stream_tx {
        let _ = tx.send(StreamEvent::Values(state.clone())).await;
    }
}

async fn emit_updates_events(
    ctx: &PregelNodeContext,
    node_ids: &[String],
    state: &ChannelValue,
) {
    if !(ctx.stream_mode.contains(&StreamMode::Updates) || ctx.stream_mode.contains(&StreamMode::Debug))
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
    if !(ctx.stream_mode.contains(&StreamMode::Checkpoints) || ctx.stream_mode.contains(&StreamMode::Debug))
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

fn next_checkpoint(
    current: &Checkpoint<ChannelValue>,
    source: CheckpointSource,
) -> Checkpoint<ChannelValue> {
    let mut checkpoint =
        Checkpoint::from_state(current.channel_values.clone(), source, current.metadata.step);
    checkpoint.channel_versions = current.channel_versions.clone();
    checkpoint.versions_seen = current.versions_seen.clone();
    checkpoint.updated_channels = current.updated_channels.clone();
    checkpoint.pending_sends = current.pending_sends.clone();
    checkpoint.pending_interrupts = current.pending_interrupts.clone();
    checkpoint.metadata.parents = current.metadata.parents.clone();
    checkpoint.metadata.children = current.metadata.children.clone();
    checkpoint
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

fn successful_node_ids(
    outcomes: &[crate::pregel::TaskOutcome],
) -> Vec<String> {
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
    let mut values_by_namespace = config.resume_values_by_namespace.clone();
    let values_by_interrupt_id = config.resume_values_by_interrupt_id.clone();
    if let Some(value) = &config.resume_value {
        values_by_namespace
            .entry(config.checkpoint_ns.clone())
            .or_insert_with(|| value.clone());
    }
    ResumeMap {
        values_by_namespace,
        values_by_interrupt_id,
    }
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

    use crate::memory::MemorySaver;
    use crate::pregel::{
        CheckpointNamespace, InMemoryPregelTaskCache, ReplayMode, ReplayRequest,
        SubgraphInvocation, SubgraphResult,
    };
    use crate::graph::{GraphInterrupt, Interrupt};
    use crate::pregel::channel::{ChannelKind, ChannelSpec};
    use crate::pregel::node::{PregelNode, PregelNodeInput, PregelNodeOutput};

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
                .emit_message_chunk(crate::stream::MessageChunk::thinking("thinking"), self.name())
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
                StreamEvent::TaskEnd { node_id, result, .. } => {
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

        let parent_state = parent_runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(parent_state.channels["out"], json!("hello"));
        let parent_task_id = crate::pregel::task_id_for(
            "pregel",
            "subgraph",
            0,
            crate::pregel::TaskKind::Pull,
        );
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

        let parent_state = parent_runtime.get_state(config.clone()).await.unwrap().unwrap();
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

        let final_parent_state = parent_runtime.get_state(config.clone()).await.unwrap().unwrap();
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
        assert_eq!(child_history.len(), 2);
        assert_eq!(
            final_parent_state
                .children
                .get(
                    final_parent_state
                        .children
                        .keys()
                        .find(|ns| ns.starts_with("parent/child/"))
                        .expect("parent state should contain linked child namespace"),
                )
                .expect("linked child checkpoints"),
            &child_history
                .iter()
                .map(|item| item.checkpoint_id.clone())
                .collect::<Vec<_>>()
        );
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
        let runtime = PregelRuntime::new(graph).with_checkpointer(checkpointer);
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

        let result = runtime.invoke(json!({"in": "hello"}), Some(config.clone())).await;
        assert!(matches!(result, Err(AgentError::Interrupted(_))));

        let state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(state.step, 0);
        assert_eq!(state.channels["in"], json!("hello"));
        assert!(state.channels.get("out").unwrap_or(&serde_json::Value::Null).is_null());

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

        let result = runtime.invoke(json!({"in": "hello"}), Some(config.clone())).await;
        assert!(matches!(result, Err(AgentError::Interrupted(_))));

        let state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(state.step, 0);
        assert_eq!(state.channels["in"], json!("hello"));
        assert!(state.channels.get("out").unwrap_or(&serde_json::Value::Null).is_null());

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

        let result = runtime.invoke(json!({"in": "hello"}), Some(config.clone())).await;
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

        let runtime = PregelRuntime::new(graph)
            .with_task_cache(Arc::new(InMemoryPregelTaskCache::new()));

        let first = runtime.invoke(json!({"in": "hello"}), None).await.unwrap();
        let second = runtime.invoke(json!({"in": "hello"}), None).await.unwrap();

        assert_eq!(first["out"], json!("hello"));
        assert_eq!(second["out"], json!("hello"));
        assert_eq!(runs.load(Ordering::SeqCst), 1);
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

        let forked = runtime
            .replay(
                RunnableConfig {
                    checkpoint_ns: "fork".to_string(),
                    ..config.clone()
                },
                ReplayRequest {
                    mode: ReplayMode::ForkFromCheckpoint(checkpoint_id),
                    namespace: Some("main".to_string()),
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert!(forked.forked);
        assert_eq!(forked.snapshot.channels["out"], json!("hello"));

        let fork_history = runtime
            .get_state_history(
                RunnableConfig {
                    checkpoint_ns: "fork".to_string(),
                    ..config
                },
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(fork_history.len(), 1);
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

        let runtime = PregelRuntime::new(graph)
            .with_managed_value("tenant", json!("acme"));
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

        let first = runtime.invoke(json!({"in": "hello"}), Some(config.clone())).await;
        assert!(matches!(first, Err(AgentError::Interrupted(_))));

        let interrupted_state = runtime.get_state(config.clone()).await.unwrap().unwrap();
        assert_eq!(interrupted_state.pending_interrupts.len(), 1);

        let resumed = runtime
            .invoke(
                json!({"in": "hello"}),
                Some(RunnableConfig {
                    resume_values_by_interrupt_id: [(
                        interrupted_state.pending_interrupts[0]
                            .interrupt_id
                            .clone(),
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
        let parent_runtime = PregelRuntime::new(parent_graph).with_checkpointer(checkpointer.clone());

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
        let parent_runtime = PregelRuntime::new(parent_graph).with_checkpointer(checkpointer.clone());

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
                return Err(AgentError::ExecutionFailed(format!("transient error #{attempt}")));
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
}
