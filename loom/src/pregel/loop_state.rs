//! Pregel loop state.
//!
//! [`PregelLoop`] is the mutable state machine that advances one Pregel run
//! through repeated barriers. It tracks the active checkpoint, channel values,
//! pending writes, and interrupt bookkeeping between calls to [`PregelLoop::tick`]
//! and [`PregelLoop::after_tick`].

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::cli_run::RunCancellation;
use crate::error::AgentError;
use crate::graph::{GraphInterrupt, Interrupt};
use crate::pregel::algo::{
    apply_writes, normalize_pending_sends, normalize_pending_writes, pending_send_packet_id,
    prepare_next_tasks, prepare_resume_tasks_from_interrupts, ExecutableTask, PreparedTask,
    TaskOutcome,
};
use crate::pregel::channel::BoxedChannel;
use crate::pregel::config::PregelConfig;
use crate::pregel::node::PregelGraph;
use crate::pregel::types::{
    ChannelName, ChannelValue, InterruptRecord, LoopStatus, PendingWrite, ReservedWrite,
};

/// Loop-level interrupt configuration and pending resume data.
#[derive(Debug, Clone, Default)]
pub struct InterruptState {
    /// Node names that should interrupt before the node executes.
    pub interrupt_before: Vec<String>,
    /// Node names that should interrupt after the node executes.
    pub interrupt_after: Vec<String>,
    /// Resume payloads recovered from checkpoint state for the next step.
    pub pending_resume_values: Vec<ChannelValue>,
    /// Interrupt ids already resumed during this loop.
    pub consumed_interrupt_ids: HashSet<String>,
}

/// Mutable runtime state for a Pregel execution.
#[derive(Debug)]
pub struct PregelLoop {
    /// Current step number.
    pub step: u64,
    /// Maximum step count before the loop stops with `OutOfSteps`.
    pub stop: u64,
    /// Current loop status.
    pub status: LoopStatus,
    /// Immutable graph definition for this run.
    pub graph: Arc<PregelGraph>,
    /// Checkpoint namespace used for persistence and subgraph isolation.
    pub checkpoint_namespace: String,
    /// Latest persisted-or-in-memory checkpoint snapshot.
    pub checkpoint: crate::memory::Checkpoint<serde_json::Value>,
    /// Materialized channel states for the current step.
    pub channels: HashMap<ChannelName, BoxedChannel>,
    /// Writes staged for the next barrier or loaded from checkpoint state.
    pub pending_writes: Vec<PendingWrite>,
    /// Channels updated at the most recent barrier.
    pub updated_channels: Vec<ChannelName>,
    /// Runtime config copied into the loop for step-local decisions.
    pub config: PregelConfig,
    /// Interrupt configuration and resume bookkeeping.
    pub interrupts: InterruptState,
}

impl PregelLoop {
    /// Creates a new loop state from graph, checkpoint, and channels.
    pub fn new(
        graph: Arc<PregelGraph>,
        checkpoint_namespace: String,
        checkpoint: crate::memory::Checkpoint<serde_json::Value>,
        channels: HashMap<ChannelName, BoxedChannel>,
        config: PregelConfig,
    ) -> Self {
        Self {
            step: checkpoint.metadata.step.max(0) as u64,
            stop: config.max_steps,
            status: LoopStatus::Running,
            graph,
            checkpoint_namespace,
            pending_writes: checkpoint.pending_writes.clone(),
            updated_channels: checkpoint.updated_channels.clone().unwrap_or_default(),
            checkpoint,
            channels,
            interrupts: InterruptState {
                interrupt_before: config.interrupt_before.clone(),
                interrupt_after: config.interrupt_after.clone(),
                pending_resume_values: Vec::new(),
                consumed_interrupt_ids: HashSet::new(),
            },
            config,
        }
    }

    /// Prepares the next step.
    ///
    /// This derives the next batch of executable tasks from updated channels
    /// and pending interrupts. It returns `Ok(None)` when the run is complete
    /// and returns [`AgentError::Interrupted`] when configured interrupts fire
    /// before task execution.
    pub async fn tick(&mut self) -> Result<Option<Vec<PreparedTask>>, AgentError> {
        if self.step >= self.stop {
            self.status = LoopStatus::OutOfSteps;
            return Ok(None);
        }

        let mut tasks = prepare_next_tasks(
            &self.checkpoint,
            &self.channels,
            &self.graph,
            self.step,
            &self.updated_channels,
        );

        if tasks.is_empty()
            && !self.checkpoint.pending_interrupts.is_empty()
            && !self.interrupts.consumed_interrupt_ids.is_empty()
        {
            tasks = prepare_resume_tasks_from_interrupts(
                &self.checkpoint,
                &self.channels,
                &self.graph,
                self.step,
                &self.interrupts.consumed_interrupt_ids,
            );
        }

        if !tasks.is_empty()
            && !self.checkpoint.pending_interrupts.is_empty()
            && !self.interrupts.consumed_interrupt_ids.is_empty()
        {
            let resume_tasks = prepare_resume_tasks_from_interrupts(
                &self.checkpoint,
                &self.channels,
                &self.graph,
                self.step,
                &self.interrupts.consumed_interrupt_ids,
            );
            let mut tasks_by_id = tasks
                .into_iter()
                .map(|task| (task.id.clone(), task))
                .collect::<std::collections::BTreeMap<_, _>>();
            for task in resume_tasks {
                tasks_by_id.entry(task.id.clone()).or_insert(task);
            }
            tasks = tasks_by_id.into_values().collect();
        }

        if tasks.is_empty() {
            self.status = LoopStatus::Done;
            return Ok(None);
        }

        let interrupt_before_tasks = tasks
            .iter()
            .filter(|task| self.interrupts.interrupt_before.iter().any(|node| node == &task.node_name))
            .collect::<Vec<_>>();
        if let Some(task) = interrupt_before_tasks.first() {
            self.status = LoopStatus::InterruptedBefore;
            let interrupt = build_configured_interrupt(task, "before");
            push_pending_interrupt_records(
                &mut self.checkpoint,
                interrupt_before_tasks
                    .iter()
                    .map(|task| {
                        let interrupt = build_configured_interrupt(task, "before");
                        interrupt_record_from_task(
                            task,
                            &interrupt.0,
                            self.checkpoint_namespace.as_str(),
                        )
                    })
                    .collect(),
                &self.interrupts.consumed_interrupt_ids,
            );
            return Err(AgentError::Interrupted(interrupt));
        }

        Ok(Some(tasks))
    }

    /// Applies step outcomes at the step barrier.
    ///
    /// This merges successful writes, records interrupts, advances checkpoint
    /// metadata, and may surface execution errors or post-step interrupts.
    pub async fn after_tick(&mut self, outcomes: Vec<TaskOutcome>) -> Result<(), AgentError> {
        let direct_interrupts = outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                TaskOutcome::Interrupted { task, interrupt } => {
                    let namespace = interrupt_namespace(&interrupt.0, self.checkpoint_namespace.as_str());
                    Some((
                        interrupt.clone(),
                        interrupt_record_from_task(&task.prepared, &interrupt.0, namespace),
                    ))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some((interrupt, _)) = direct_interrupts.first() {
            push_pending_interrupt_records(
                &mut self.checkpoint,
                direct_interrupts
                    .iter()
                    .map(|(_, record)| record.clone())
                    .collect(),
                &self.interrupts.consumed_interrupt_ids,
            );
            stage_successful_task_writes(&mut self.checkpoint, &outcomes, &HashSet::new());
            self.status = LoopStatus::InterruptedAfter;
            return Err(AgentError::Interrupted(interrupt.clone()));
        }

        let reserved_interrupts = outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                TaskOutcome::Success { task } => {
                    reserved_interrupt_from_task(task, self.checkpoint_namespace.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some((interrupt, _)) = reserved_interrupts.first() {
            let excluded_task_ids = reserved_interrupts
                .iter()
                .map(|(_, record)| record.task_id.clone())
                .collect::<HashSet<_>>();
            push_pending_interrupt_records(
                &mut self.checkpoint,
                reserved_interrupts
                    .iter()
                    .map(|(_, record)| record.clone())
                    .collect(),
                &self.interrupts.consumed_interrupt_ids,
            );
            stage_successful_task_writes(
                &mut self.checkpoint,
                &outcomes,
                &excluded_task_ids,
            );
            self.status = LoopStatus::InterruptedAfter;
            return Err(AgentError::Interrupted(interrupt.clone()));
        }

        if let Some((task_id, error)) = outcomes.iter().find_map(|outcome| match outcome {
            TaskOutcome::Success { task } => {
                reserved_error_from_task(task).map(|error| (task.prepared.id.clone(), error))
            }
            _ => None,
        }) {
            let excluded_task_ids = [task_id].into_iter().collect::<HashSet<_>>();
            stage_successful_task_writes(&mut self.checkpoint, &outcomes, &excluded_task_ids);
            self.status = LoopStatus::Failed;
            return Err(AgentError::ExecutionFailed(error));
        }

        let interrupt_after_tasks = outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                TaskOutcome::Success { task }
                    if self
                        .interrupts
                        .interrupt_after
                        .iter()
                        .any(|node| node == &task.prepared.node_name) =>
                {
                    Some(task.prepared.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some(task) = interrupt_after_tasks.first() {
            self.status = LoopStatus::InterruptedAfter;
            let interrupt = build_configured_interrupt(task, "after");
            push_pending_interrupt_records(
                &mut self.checkpoint,
                interrupt_after_tasks
                    .iter()
                    .map(|task| {
                        let interrupt = build_configured_interrupt(task, "after");
                        interrupt_record_from_task(
                            task,
                            &interrupt.0,
                            self.checkpoint_namespace.as_str(),
                        )
                    })
                    .collect(),
                &self.interrupts.consumed_interrupt_ids,
            );
            stage_successful_task_writes(&mut self.checkpoint, &outcomes, &HashSet::new());
            return Err(AgentError::Interrupted(interrupt));
        }

        if outcomes
            .iter()
            .any(|outcome| matches!(outcome, TaskOutcome::Cancelled { .. }))
        {
            stage_successful_task_writes(&mut self.checkpoint, &outcomes, &HashSet::new());
            self.status = LoopStatus::Cancelled;
            return Err(AgentError::Cancelled);
        }

        if let Some(error) = outcomes.iter().find_map(|outcome| match outcome {
            TaskOutcome::Failed { error, .. } => Some(error.to_string()),
            _ => None,
        }) {
            stage_successful_task_writes(&mut self.checkpoint, &outcomes, &HashSet::new());
            self.status = LoopStatus::Failed;
            return Err(AgentError::ExecutionFailed(error));
        }

        let tasks = outcomes
            .into_iter()
            .map(|outcome| match outcome {
                TaskOutcome::Success { task } => task,
                TaskOutcome::Interrupted { task, .. } => task,
                TaskOutcome::Cancelled { task } => task,
                TaskOutcome::Failed { task, .. } => task,
            })
            .collect::<Vec<_>>();

        let existing_pending_sends = self.checkpoint.pending_sends.clone();
        let existing_pending_writes = self.checkpoint.pending_writes.clone();
        let updated = apply_writes(
            &mut self.checkpoint,
            &mut self.channels,
            &tasks,
            &self.graph,
            |current| {
                let next = current.and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) + 1;
                next.to_string()
            },
        );
        let new_pending_sends = std::mem::take(&mut self.checkpoint.pending_sends);
        let new_pending_writes = std::mem::take(&mut self.checkpoint.pending_writes);
        self.checkpoint.pending_sends = merge_pending_sends_after_commit(
            existing_pending_sends,
            &tasks,
            new_pending_sends,
        );
        self.checkpoint.pending_writes = merge_pending_writes_after_commit(
            existing_pending_writes,
            &self.checkpoint.pending_interrupts,
            &self.interrupts.consumed_interrupt_ids,
            &tasks,
            new_pending_writes,
        );
        self.updated_channels = updated;
        self.pending_writes = self.checkpoint.pending_writes.clone();
        self.step += 1;
        self.checkpoint.metadata.step = self.step as i64;
        if !self.interrupts.consumed_interrupt_ids.is_empty() {
            self.checkpoint.pending_interrupts = retain_unconsumed_interrupts(
                &self.checkpoint.pending_interrupts,
                &self.interrupts.consumed_interrupt_ids,
            );
        }
        Ok(())
    }

    /// Returns the latest output snapshot.
    ///
    /// This is the raw checkpoint state after the most recent barrier.
    pub fn output(&self) -> ChannelValue {
        self.checkpoint.channel_values.clone()
    }

    /// Returns the value surfaced to callers after the run completes.
    ///
    /// If a reserved return write is present, that value wins; otherwise the
    /// full output snapshot is returned.
    pub fn final_output(&self) -> ChannelValue {
        reserved_return_output(&self.checkpoint.pending_writes)
            .unwrap_or_else(|| self.output())
    }

    /// Returns whether the cancellation token for this run has fired.
    pub fn is_cancelled(cancellation: Option<&RunCancellation>) -> bool {
        cancellation
            .map(|c| c.token().is_cancelled())
            .unwrap_or(false)
    }

    /// Returns whether a channel name is a reserved write channel.
    ///
    /// Reserved channels are used for runtime control flow rather than normal
    /// user-defined state propagation.
    pub fn is_reserved_write(channel: &str) -> bool {
        [
            ReservedWrite::Error.as_str(),
            ReservedWrite::Interrupt.as_str(),
            ReservedWrite::Resume.as_str(),
            ReservedWrite::Scheduled.as_str(),
            ReservedWrite::Push.as_str(),
            ReservedWrite::Return.as_str(),
            ReservedWrite::NoWrites.as_str(),
            ReservedWrite::Tasks.as_str(),
        ]
        .contains(&channel)
    }
}

fn build_configured_interrupt(task: &PreparedTask, when: &str) -> GraphInterrupt {
    let interrupt = Interrupt::with_id(
        serde_json::json!({
            "kind": "pregel_interrupt",
            "when": when,
            "node": task.node_name,
            "task_id": task.id,
            "step": task.step,
        }),
        format!("pregel:{}:{}:{}", when, task.node_name, task.step),
    );
    GraphInterrupt(interrupt)
}

fn interrupt_record_from_task(
    task: &PreparedTask,
    interrupt: &Interrupt,
    namespace: &str,
) -> InterruptRecord {
    InterruptRecord {
        interrupt_id: interrupt
            .id
            .clone()
            .unwrap_or_else(|| format!("pregel:{}:{}", task.node_name, task.step)),
        namespace: namespace.to_string(),
        task_id: task.id.clone(),
        node_name: task.node_name.clone(),
        step: task.step,
        value: interrupt.value.clone(),
    }
}

fn interrupt_namespace<'a>(interrupt: &'a Interrupt, fallback: &'a str) -> &'a str {
    interrupt
        .value
        .get("namespace")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback)
}

fn reserved_interrupt_from_task(
    task: &ExecutableTask,
    default_namespace: &str,
) -> Option<(GraphInterrupt, InterruptRecord)> {
    let value = task
        .writes
        .iter()
        .find(|(channel, _)| channel == ReservedWrite::Interrupt.as_str())
        .map(|(_, value)| value.clone())?;
    let interrupt_id = value
        .get("interrupt_id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("id").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "pregel:interrupt-write:{}:{}",
                task.prepared.node_name, task.prepared.step
            )
        });
    let interrupt = GraphInterrupt(Interrupt::with_id(value.clone(), interrupt_id));
    let namespace = interrupt_namespace(&interrupt.0, default_namespace);
    let record = interrupt_record_from_task(&task.prepared, &interrupt.0, namespace);
    Some((interrupt, record))
}

fn reserved_error_from_task(task: &ExecutableTask) -> Option<String> {
    let value = task
        .writes
        .iter()
        .find(|(channel, _)| channel == ReservedWrite::Error.as_str())
        .map(|(_, value)| value)?;
    Some(match value {
        serde_json::Value::String(message) => message.clone(),
        serde_json::Value::Object(map) => map
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string()),
        _ => value.to_string(),
    })
}

fn reserved_return_output(pending_writes: &[PendingWrite]) -> Option<ChannelValue> {
    let mut values = pending_writes
        .iter()
        .filter(|(_, channel, _)| channel == ReservedWrite::Return.as_str())
        .map(|(_, _, value)| value.clone())
        .collect::<Vec<_>>();
    match values.len() {
        0 => None,
        1 => values.pop(),
        _ => Some(ChannelValue::Array(values)),
    }
}

fn retain_unconsumed_interrupts(
    pending_interrupts: &[serde_json::Value],
    consumed_interrupt_ids: &HashSet<String>,
) -> Vec<serde_json::Value> {
    pending_interrupts
        .iter()
        .filter_map(|value| {
            let record = serde_json::from_value::<InterruptRecord>(value.clone()).ok()?;
            if consumed_interrupt_ids.contains(record.interrupt_id.as_str()) {
                return None;
            }
            Some(serde_json::to_value(record).expect("interrupt record serializes"))
        })
        .collect()
}

fn push_pending_interrupt_record(
    checkpoint: &mut crate::memory::Checkpoint<serde_json::Value>,
    record: InterruptRecord,
    consumed_interrupt_ids: &HashSet<String>,
) {
    let mut pending_interrupts =
        retain_unconsumed_interrupts(&checkpoint.pending_interrupts, consumed_interrupt_ids);
    if pending_interrupts.iter().any(|value| {
        serde_json::from_value::<InterruptRecord>(value.clone())
            .map(|existing| existing.interrupt_id == record.interrupt_id)
            .unwrap_or(false)
    }) {
        checkpoint.pending_interrupts = pending_interrupts;
        return;
    }
    pending_interrupts.push(serde_json::to_value(record).expect("interrupt record serializes"));
    checkpoint.pending_interrupts = pending_interrupts;
}

fn push_pending_interrupt_records(
    checkpoint: &mut crate::memory::Checkpoint<serde_json::Value>,
    records: Vec<InterruptRecord>,
    consumed_interrupt_ids: &HashSet<String>,
) {
    for record in records {
        push_pending_interrupt_record(checkpoint, record, consumed_interrupt_ids);
    }
}

fn stage_successful_task_writes(
    checkpoint: &mut crate::memory::Checkpoint<serde_json::Value>,
    outcomes: &[TaskOutcome],
    excluded_task_ids: &HashSet<String>,
) {
    let mut pending_writes = checkpoint.pending_writes.clone();
    for outcome in outcomes {
        let TaskOutcome::Success { task } = outcome else {
            continue;
        };
        if excluded_task_ids.contains(task.prepared.id.as_str()) {
            continue;
        }
        for (channel, value) in &task.writes {
            pending_writes.push((task.prepared.id.clone(), channel.clone(), value.clone()));
        }
    }
    normalize_pending_writes(&mut pending_writes);
    checkpoint.pending_writes = pending_writes;
}

fn merge_pending_sends_after_commit(
    existing_pending_sends: Vec<PendingWrite>,
    committed_tasks: &[ExecutableTask],
    new_pending_sends: Vec<PendingWrite>,
) -> Vec<PendingWrite> {
    let consumed_packet_ids = committed_tasks
        .iter()
        .filter_map(|task| task.prepared.packet_id.clone())
        .collect::<HashSet<_>>();
    let mut pending_sends = existing_pending_sends
        .into_iter()
        .filter(|(_, _, value)| {
            pending_send_packet_id(value)
                .map(|packet_id| !consumed_packet_ids.contains(packet_id.as_str()))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    pending_sends.extend(new_pending_sends);
    normalize_pending_sends(&mut pending_sends);
    pending_sends
}

fn merge_pending_writes_after_commit(
    existing_pending_writes: Vec<PendingWrite>,
    pending_interrupts: &[serde_json::Value],
    consumed_interrupt_ids: &HashSet<String>,
    committed_tasks: &[ExecutableTask],
    new_pending_writes: Vec<PendingWrite>,
) -> Vec<PendingWrite> {
    let committed_task_ids = committed_tasks
        .iter()
        .map(|task| task.prepared.id.clone())
        .collect::<HashSet<_>>();
    let consumed_interrupt_records = pending_interrupts
        .iter()
        .filter_map(|value| serde_json::from_value::<InterruptRecord>(value.clone()).ok())
        .filter(|record| consumed_interrupt_ids.contains(record.interrupt_id.as_str()))
        .collect::<Vec<_>>();
    let mut pending_writes = existing_pending_writes
        .into_iter()
        .filter(|(task_id, channel, value)| {
            if committed_task_ids.contains(task_id.as_str()) {
                return false;
            }
            if channel != ReservedWrite::Resume.as_str() {
                return true;
            }
            !resume_write_matches_consumed_interrupts(value, &consumed_interrupt_records)
        })
        .collect::<Vec<_>>();
    pending_writes.extend(new_pending_writes);
    normalize_pending_writes(&mut pending_writes);
    pending_writes
}

fn resume_write_matches_consumed_interrupts(
    value: &ChannelValue,
    consumed_interrupt_records: &[InterruptRecord],
) -> bool {
    if consumed_interrupt_records.is_empty() {
        return false;
    }
    let consumed_interrupt_ids = consumed_interrupt_records
        .iter()
        .map(|record| record.interrupt_id.as_str())
        .collect::<HashSet<_>>();
    let consumed_namespaces = consumed_interrupt_records
        .iter()
        .map(|record| record.namespace.as_str())
        .collect::<HashSet<_>>();

    if let Some(map) = value.as_object() {
        if let Some(interrupt_id) = map.get("interrupt_id").and_then(serde_json::Value::as_str) {
            return consumed_interrupt_ids.contains(interrupt_id);
        }
        if let Some(namespace) = map.get("namespace").and_then(serde_json::Value::as_str) {
            return consumed_namespaces.contains(namespace);
        }
    }

    consumed_interrupt_records.len() == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pregel::channel::{ChannelKind, ChannelSpec};

    #[test]
    fn loop_new_uses_checkpoint_step() {
        let mut graph = PregelGraph::new();
        graph.add_channel("a", ChannelSpec::new(ChannelKind::LastValue));
        let mut checkpoint = crate::memory::Checkpoint::from_state(
            serde_json::json!({}),
            crate::memory::CheckpointSource::Loop,
            3,
        );
        checkpoint.updated_channels = Some(vec!["a".to_string()]);
        let loop_state = PregelLoop::new(
            Arc::new(graph),
            String::new(),
            checkpoint,
            HashMap::new(),
            PregelConfig::default(),
        );
        assert_eq!(loop_state.step, 3);
        assert_eq!(loop_state.stop, 100);
        assert_eq!(loop_state.updated_channels, vec!["a".to_string()]);
    }

    #[test]
    fn final_output_prefers_reserved_return_value() {
        let mut graph = PregelGraph::new();
        graph.add_channel("a", ChannelSpec::new(ChannelKind::LastValue));
        let mut checkpoint = crate::memory::Checkpoint::from_state(
            serde_json::json!({"out": "channel"}),
            crate::memory::CheckpointSource::Loop,
            1,
        );
        checkpoint.pending_writes.push((
            "task-1".to_string(),
            ReservedWrite::Return.as_str().to_string(),
            serde_json::json!({"final": true}),
        ));
        let loop_state = PregelLoop::new(
            Arc::new(graph),
            String::new(),
            checkpoint,
            HashMap::new(),
            PregelConfig::default(),
        );

        assert_eq!(loop_state.final_output(), serde_json::json!({"final": true}));
    }
}
