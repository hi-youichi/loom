//! Pregel loop state.

use std::collections::HashMap;
use std::sync::Arc;

use crate::cli_run::RunCancellation;
use crate::error::AgentError;
use crate::graph::{GraphInterrupt, Interrupt};
use crate::pregel::algo::{apply_writes, prepare_next_tasks, PreparedTask, TaskOutcome};
use crate::pregel::channel::BoxedChannel;
use crate::pregel::config::PregelConfig;
use crate::pregel::node::PregelGraph;
use crate::pregel::types::{
    ChannelName, ChannelValue, InterruptRecord, LoopStatus, PendingWrite, ReservedWrite,
};

/// Loop-level interrupt configuration and pending resume data.
#[derive(Debug, Clone, Default)]
pub struct InterruptState {
    pub interrupt_before: Vec<String>,
    pub interrupt_after: Vec<String>,
    pub pending_resume_values: Vec<ChannelValue>,
}

/// Mutable runtime state for a Pregel execution.
#[derive(Debug)]
pub struct PregelLoop {
    pub step: u64,
    pub stop: u64,
    pub status: LoopStatus,
    pub graph: Arc<PregelGraph>,
    pub checkpoint_namespace: String,
    pub checkpoint: crate::memory::Checkpoint<serde_json::Value>,
    pub channels: HashMap<ChannelName, BoxedChannel>,
    pub pending_writes: Vec<PendingWrite>,
    pub updated_channels: Vec<ChannelName>,
    pub config: PregelConfig,
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
            checkpoint,
            channels,
            pending_writes: Vec::new(),
            updated_channels: Vec::new(),
            interrupts: InterruptState {
                interrupt_before: config.interrupt_before.clone(),
                interrupt_after: config.interrupt_after.clone(),
                pending_resume_values: Vec::new(),
            },
            config,
        }
    }

    /// Prepares the next step. The execution path is intentionally stubbed for now.
    pub async fn tick(&mut self) -> Result<Option<Vec<PreparedTask>>, AgentError> {
        if self.step >= self.stop {
            self.status = LoopStatus::OutOfSteps;
            return Ok(None);
        }

        let tasks = prepare_next_tasks(
            &self.checkpoint,
            &self.channels,
            &self.graph,
            self.step,
            &self.updated_channels,
        );

        if tasks.is_empty() {
            self.status = LoopStatus::Done;
            return Ok(None);
        }

        if let Some(task) = tasks
            .iter()
            .find(|task| self.interrupts.interrupt_before.iter().any(|node| node == &task.node_name))
        {
            self.status = LoopStatus::InterruptedBefore;
            let interrupt = build_configured_interrupt(task, "before");
            self.checkpoint.pending_interrupts = vec![serde_json::to_value(
                interrupt_record_from_task(
                    task,
                    &interrupt.0,
                    self.checkpoint_namespace.as_str(),
                ),
            )
            .expect("interrupt record serializes")];
            return Err(AgentError::Interrupted(interrupt));
        }

        Ok(Some(tasks))
    }

    /// Applies step outcomes at the step barrier.
    pub async fn after_tick(&mut self, outcomes: Vec<TaskOutcome>) -> Result<(), AgentError> {
        if let Some(interrupt) = outcomes.iter().find_map(|outcome| match outcome {
            TaskOutcome::Interrupted { task, interrupt } => {
                let namespace = interrupt_namespace(&interrupt.0, self.checkpoint_namespace.as_str());
                self.checkpoint.pending_interrupts = vec![serde_json::to_value(
                    interrupt_record_from_task(&task.prepared, &interrupt.0, namespace),
                )
                .expect("interrupt record serializes")];
                Some(interrupt.clone())
            }
            _ => None,
        }) {
            self.status = LoopStatus::InterruptedAfter;
            return Err(AgentError::Interrupted(interrupt));
        }

        if let Some(task) = outcomes.iter().find_map(|outcome| match outcome {
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
        }) {
            self.status = LoopStatus::InterruptedAfter;
            let interrupt = build_configured_interrupt(&task, "after");
            self.checkpoint.pending_interrupts = vec![serde_json::to_value(
                interrupt_record_from_task(
                    &task,
                    &interrupt.0,
                    self.checkpoint_namespace.as_str(),
                ),
            )
            .expect("interrupt record serializes")];
            return Err(AgentError::Interrupted(interrupt));
        }

        if outcomes
            .iter()
            .any(|outcome| matches!(outcome, TaskOutcome::Cancelled { .. }))
        {
            self.status = LoopStatus::Cancelled;
            return Err(AgentError::Cancelled);
        }

        if let Some(error) = outcomes.iter().find_map(|outcome| match outcome {
            TaskOutcome::Failed { error, .. } => Some(error.to_string()),
            _ => None,
        }) {
            self.status = LoopStatus::Failed;
            return Err(AgentError::ExecutionFailed(error));
        }

        let tasks = outcomes
            .into_iter()
            .filter_map(|outcome| match outcome {
                TaskOutcome::Success { task } => Some(task),
                TaskOutcome::Interrupted { task, .. } => Some(task),
                TaskOutcome::Cancelled { task } => Some(task),
                TaskOutcome::Failed { task, .. } => Some(task),
            })
            .collect::<Vec<_>>();

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
        self.updated_channels = updated;
        self.step += 1;
        self.checkpoint.metadata.step = self.step as i64;
        if !self.interrupts.pending_resume_values.is_empty() {
            self.checkpoint.pending_interrupts.clear();
        }
        Ok(())
    }

    /// Returns the latest output snapshot.
    pub fn output(&self) -> ChannelValue {
        self.checkpoint.channel_values.clone()
    }

    /// Returns whether the cancellation token for this run has fired.
    pub fn is_cancelled(cancellation: Option<&RunCancellation>) -> bool {
        cancellation
            .map(|c| c.token().is_cancelled())
            .unwrap_or(false)
    }

    /// Returns whether a channel name is a reserved write channel.
    pub fn is_reserved_write(channel: &str) -> bool {
        [
            ReservedWrite::Error.as_str(),
            ReservedWrite::Interrupt.as_str(),
            ReservedWrite::Resume.as_str(),
            ReservedWrite::Scheduled.as_str(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pregel::channel::{ChannelKind, ChannelSpec};

    #[test]
    fn loop_new_uses_checkpoint_step() {
        let mut graph = PregelGraph::new();
        graph.add_channel("a", ChannelSpec::new(ChannelKind::LastValue));
        let checkpoint = crate::memory::Checkpoint::from_state(
            serde_json::json!({}),
            crate::memory::CheckpointSource::Loop,
            3,
        );
        let loop_state = PregelLoop::new(
            Arc::new(graph),
            String::new(),
            checkpoint,
            HashMap::new(),
            PregelConfig::default(),
        );
        assert_eq!(loop_state.step, 3);
        assert_eq!(loop_state.stop, 100);
    }
}
