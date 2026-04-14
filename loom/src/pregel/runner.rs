//! Pregel task runner.

use std::sync::Arc;

use crate::error::AgentError;
use crate::pregel::algo::{ExecutableTask, PreparedTask, TaskOutcome};
use crate::pregel::node::{PregelGraph, PregelNodeContext};
use crate::stream::{StreamEvent, StreamMode};

/// Executes a frontier of prepared tasks.
#[derive(Debug, Clone)]
pub struct PregelRunner {
    pub retry_policy: crate::graph::RetryPolicy,
}

impl PregelRunner {
    /// Creates a new runner.
    pub fn new(retry_policy: crate::graph::RetryPolicy) -> Self {
        Self { retry_policy }
    }

    /// Runs one step worth of tasks.
    pub async fn run_step(
        &self,
        tasks: Vec<PreparedTask>,
        graph: Arc<PregelGraph>,
        ctx: PregelNodeContext,
    ) -> Vec<TaskOutcome> {
        if is_cancelled(&ctx) {
            return vec![cancelled_outcome(tasks.first().cloned())];
        }
        let mut outcomes = Vec::with_capacity(tasks.len());
        let mut join_set = tokio::task::JoinSet::new();
        let cancel_fallback_task = tasks.first().cloned();

        for task in tasks {
            if is_cancelled(&ctx) {
                join_set.abort_all();
                return vec![cancelled_outcome(cancel_fallback_task)];
            }
            if should_emit_task_events(&ctx) {
                if let Some(tx) = &ctx.stream_tx {
                    let _ = tx
                        .send(StreamEvent::TaskStart {
                            node_id: task.node_name.clone(),
                            namespace: if ctx.run_config.checkpoint_ns.is_empty() {
                                None
                            } else {
                                Some(ctx.run_config.checkpoint_ns.clone())
                            },
                        })
                        .await;
                }
            }

            let graph = Arc::clone(&graph);
            let ctx = ctx.clone();
            let runner = self.clone();
            join_set.spawn(async move { runner.run_task(task, graph, &ctx).await });
        }

        loop {
            let result = if let Some(cancellation) = ctx.cancellation.as_ref() {
                let token = cancellation.token();
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        join_set.abort_all();
                        outcomes.push(cancelled_outcome(cancel_fallback_task.clone()));
                        break;
                    }
                    result = join_set.join_next() => result,
                }
            } else {
                join_set.join_next().await
            };
            let Some(result) = result else {
                break;
            };
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(join_error) => TaskOutcome::Failed {
                    task: ExecutableTask {
                        prepared: PreparedTask {
                            id: "join-error".to_string(),
                            kind: crate::pregel::TaskKind::Pull,
                            node_name: "join-error".to_string(),
                            step: 0,
                            triggers: Vec::new(),
                            input: serde_json::Value::Null,
                            packet_id: None,
                            origin_task_id: None,
                            cached_writes: Vec::new(),
                        },
                        writes: Vec::new(),
                        attempt: 0,
                    },
                    error: AgentError::ExecutionFailed(join_error.to_string()),
                },
            };

            if should_emit_task_events(&ctx) {
                if let Some(tx) = &ctx.stream_tx {
                    let node_id = match &outcome {
                        TaskOutcome::Success { task }
                        | TaskOutcome::Interrupted { task, .. }
                        | TaskOutcome::Cancelled { task }
                        | TaskOutcome::Failed { task, .. } => task.prepared.node_name.clone(),
                    };
                    let result = match &outcome {
                        TaskOutcome::Success { .. } => Ok(()),
                        TaskOutcome::Interrupted { interrupt, .. } => {
                            Err(format!("interrupted: {}", interrupt))
                        }
                        TaskOutcome::Cancelled { .. } => Err("cancelled".to_string()),
                        TaskOutcome::Failed { error, .. } => Err(error.to_string()),
                    };
                    let _ = tx
                        .send(StreamEvent::TaskEnd {
                            node_id,
                            result,
                            namespace: if ctx.run_config.checkpoint_ns.is_empty() {
                                None
                            } else {
                                Some(ctx.run_config.checkpoint_ns.clone())
                            },
                        })
                        .await;
                }
            }

            let stop_early = matches!(
                outcome,
                TaskOutcome::Cancelled { .. } | TaskOutcome::Failed { .. }
            );
            outcomes.push(outcome);
            if stop_early {
                join_set.abort_all();
                break;
            }
        }

        outcomes
    }

    /// Runs a single task with retry support.
    pub async fn run_task(
        &self,
        prepared: PreparedTask,
        graph: Arc<PregelGraph>,
        ctx: &PregelNodeContext,
    ) -> TaskOutcome {
        if is_cancelled(ctx) {
            return TaskOutcome::Cancelled {
                task: ExecutableTask {
                    writes: Vec::new(),
                    prepared,
                    attempt: 0,
                },
            };
        }
        if !prepared.cached_writes.is_empty() {
            return TaskOutcome::Success {
                task: ExecutableTask {
                    writes: prepared.cached_writes.clone(),
                    prepared,
                    attempt: 0,
                },
            };
        }

        let Some(node) = graph.nodes.get(&prepared.node_name).cloned() else {
            return TaskOutcome::Failed {
                task: ExecutableTask {
                    writes: Vec::new(),
                    prepared: prepared.clone(),
                    attempt: 0,
                },
                error: AgentError::ExecutionFailed(format!(
                    "pregel node not found: {}",
                    prepared.node_name
                )),
            };
        };

        let input = build_node_input(&prepared, ctx);

        let mut attempt: u32 = 0;
        loop {
            if is_cancelled(ctx) {
                return TaskOutcome::Cancelled {
                    task: ExecutableTask {
                        writes: Vec::new(),
                        prepared,
                        attempt,
                    },
                };
            }
            match node.run(input.clone(), ctx).await {
                Ok(output) => {
                    if is_cancelled(ctx) {
                        return TaskOutcome::Cancelled {
                            task: ExecutableTask {
                                writes: Vec::new(),
                                prepared,
                                attempt,
                            },
                        };
                    }
                    return TaskOutcome::Success {
                        task: ExecutableTask {
                            writes: output.writes,
                            prepared,
                            attempt,
                        },
                    };
                }
                Err(AgentError::Interrupted(interrupt)) => {
                    return TaskOutcome::Interrupted {
                        task: ExecutableTask {
                            writes: Vec::new(),
                            prepared,
                            attempt,
                        },
                        interrupt,
                    };
                }
                Err(error) => {
                    if self.retry_policy.should_retry(attempt as usize) {
                        let delay = self.retry_policy.delay(attempt as usize);
                        if !delay.is_zero() {
                            tokio::time::sleep(delay).await;
                        }
                        attempt += 1;
                        continue;
                    }
                    return TaskOutcome::Failed {
                        task: ExecutableTask {
                            writes: Vec::new(),
                            prepared,
                            attempt,
                        },
                        error,
                    };
                }
            }
        }
    }

    /// Aborts inflight tasks. Currently a no-op since tasks run sequentially
    /// via `JoinSet`; will be extended when parallel scheduling is added.
    #[allow(dead_code)]
    pub fn abort_inflight(&self) {}
}

fn build_node_input(
    prepared: &PreparedTask,
    ctx: &PregelNodeContext,
) -> crate::pregel::node::PregelNodeInput {
    crate::pregel::node::PregelNodeInput {
        step: prepared.step,
        trigger_values: match &prepared.input {
            serde_json::Value::Object(map) => prepared
                .triggers
                .iter()
                .filter_map(|trigger| {
                    map.get(trigger)
                        .cloned()
                        .map(|value| (trigger.clone(), value))
                })
                .collect(),
            _ => Default::default(),
        },
        read_values: match &prepared.input {
            serde_json::Value::Object(map) => map.clone().into_iter().collect(),
            _ => Default::default(),
        },
        managed_values: ctx.managed_values.clone(),
        local_read_values: match &prepared.input {
            serde_json::Value::Object(map) => map.clone().into_iter().collect(),
            _ => Default::default(),
        },
        scratchpad: crate::pregel::PregelScratchpad {
            task_id: prepared.id.clone(),
            resume_value: resolve_resume_value(prepared, ctx),
            interrupt_counter: u32::from(!ctx.pending_interrupts.is_empty()),
            local_state: Default::default(),
        },
    }
}

fn should_emit_task_events(ctx: &PregelNodeContext) -> bool {
    ctx.stream_mode.contains(&StreamMode::Tasks) || ctx.stream_mode.contains(&StreamMode::Debug)
}

fn is_cancelled(ctx: &PregelNodeContext) -> bool {
    ctx.cancellation
        .as_ref()
        .map(|cancellation| cancellation.token().is_cancelled())
        .unwrap_or(false)
}

fn cancelled_outcome(prepared: Option<PreparedTask>) -> TaskOutcome {
    TaskOutcome::Cancelled {
        task: ExecutableTask {
            writes: Vec::new(),
            prepared: prepared.unwrap_or_else(|| PreparedTask {
                id: "cancelled".to_string(),
                kind: crate::pregel::TaskKind::Pull,
                node_name: "cancelled".to_string(),
                step: 0,
                triggers: Vec::new(),
                input: serde_json::Value::Null,
                packet_id: None,
                origin_task_id: None,
                cached_writes: Vec::new(),
            }),
            attempt: 0,
        },
    }
}

fn resolve_resume_value(
    prepared: &PreparedTask,
    ctx: &PregelNodeContext,
) -> Option<serde_json::Value> {
    ctx.pending_interrupts
        .iter()
        .find_map(|record| {
            if record.task_id != prepared.id {
                return None;
            }
            ctx.resume_map
                .values_by_interrupt_id
                .get(&record.interrupt_id)
                .cloned()
                .or_else(|| {
                    ctx.resume_map
                        .values_by_namespace
                        .get(&record.namespace)
                        .cloned()
                })
        })
        .or_else(|| {
            if ctx.pending_interrupts.len() != 1 {
                return None;
            }
            let record = &ctx.pending_interrupts[0];
            if record.node_name != prepared.node_name {
                return None;
            }
            ctx.resume_map
                .values_by_interrupt_id
                .get(&record.interrupt_id)
                .cloned()
                .or_else(|| {
                    ctx.resume_map
                        .values_by_namespace
                        .get(&record.namespace)
                        .cloned()
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_new_captures_policy() {
        let runner = PregelRunner::new(crate::graph::RetryPolicy::default());
        assert!(matches!(
            runner.retry_policy,
            crate::graph::RetryPolicy::None
        ));
    }
}
