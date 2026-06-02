//! Core Pregel algorithms.

use std::collections::{hash_map::DefaultHasher, BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};

use loom_graph::memory::RunnableConfig;
use crate::cache::TaskCacheKey;
use crate::channel::{build_channel, BoxedChannel};
use crate::node::PregelGraph;
use crate::types::{
    ChannelName, ChannelValue, ChannelVersion, InterruptRecord, ReservedWrite, SendPacket, TaskId,
    TaskKind, TASKS_CHANNEL,
};

/// A task prepared for execution in the next Pregel step.
#[derive(Debug, Clone)]
pub struct PreparedTask {
    pub id: TaskId,
    pub kind: TaskKind,
    pub node_name: String,
    pub step: u64,
    pub triggers: Vec<ChannelName>,
    pub input: ChannelValue,
    pub packet_id: Option<String>,
    pub origin_task_id: Option<TaskId>,
    pub cached_writes: Vec<(ChannelName, ChannelValue)>,
}

/// A task that is currently executing or has completed.
#[derive(Debug, Clone)]
pub struct ExecutableTask {
    pub prepared: PreparedTask,
    pub writes: Vec<(ChannelName, ChannelValue)>,
    pub attempt: u32,
}

/// Outcome of one task execution.
#[derive(Debug)]
pub enum TaskOutcome {
    Success {
        task: ExecutableTask,
    },
    Interrupted {
        task: ExecutableTask,
        interrupt: loom_graph::Interrupt,
    },
    Cancelled {
        task: ExecutableTask,
    },
    Failed {
        task: ExecutableTask,
        error: loom_llm::AgentError,
    },
}

/// Prepares the next set of tasks from updated channels.
pub fn prepare_next_tasks(
    checkpoint: &loom_graph::memory::Checkpoint<serde_json::Value>,
    channels: &HashMap<ChannelName, BoxedChannel>,
    graph: &PregelGraph,
    step: u64,
    updated_channels: &[ChannelName],
) -> Vec<PreparedTask> {
    let mut tasks_by_id: BTreeMap<TaskId, PreparedTask> = BTreeMap::new();
    let updated: std::collections::HashSet<&str> =
        updated_channels.iter().map(String::as_str).collect();

    prepare_pull_tasks(&mut tasks_by_id, channels, graph, step, &updated);
    prepare_push_tasks(&mut tasks_by_id, checkpoint, graph, step);

    tasks_by_id.into_values().collect()
}

/// Normalizes persisted pending send records using packet identity semantics.
pub fn normalize_pending_sends(pending_sends: &mut Vec<(TaskId, ChannelName, ChannelValue)>) {
    let mut normalized = Vec::with_capacity(pending_sends.len());
    for (task_id, channel_name, value) in pending_sends.drain(..) {
        if channel_name != TASKS_CHANNEL {
            normalized.push((task_id, channel_name, value));
            continue;
        }
        if let Some(packet) = decode_send_packet(value.clone(), None, 0) {
            push_unique_pending_send(&mut normalized, task_id, packet);
        } else {
            normalized.push((task_id, channel_name, value));
        }
    }
    *pending_sends = normalized;
}

/// Normalizes persisted pending reserved writes.
///
/// Most pending writes should preserve multiple entries per task/channel so replay can
/// faithfully reproduce multi-write task outputs (for example multiple scheduled packets
/// or multiple topic-channel writes). A small set of singleton control writes keeps
/// last-write-wins semantics by task/channel.
pub fn normalize_pending_writes(pending_writes: &mut Vec<(TaskId, ChannelName, ChannelValue)>) {
    let mut normalized = Vec::with_capacity(pending_writes.len());
    for (task_id, channel_name, value) in pending_writes.drain(..) {
        push_unique_pending_write(&mut normalized, task_id, channel_name, value);
    }
    *pending_writes = normalized;
}

/// Extracts a stable packet id from a pending send record value.
pub fn pending_send_packet_id(value: &ChannelValue) -> Option<String> {
    decode_send_packet(value.clone(), None, 0).map(|packet| packet.id)
}

/// Rebuilds interrupted tasks when a checkpoint already carries resume values.
pub fn prepare_resume_tasks_from_interrupts(
    checkpoint: &loom_graph::memory::Checkpoint<serde_json::Value>,
    channels: &HashMap<ChannelName, BoxedChannel>,
    graph: &PregelGraph,
    step: u64,
    resume_interrupt_ids: &HashSet<String>,
) -> Vec<PreparedTask> {
    checkpoint
        .pending_interrupts
        .iter()
        .filter_map(|value| serde_json::from_value::<InterruptRecord>(value.clone()).ok())
        .filter(|record| resume_interrupt_ids.contains(record.interrupt_id.as_str()))
        .filter_map(|record| {
            let node = graph.nodes.get(&record.node_name)?;
            Some(PreparedTask {
                id: record.task_id,
                kind: TaskKind::Pull,
                node_name: record.node_name,
                step,
                triggers: node.triggers().to_vec(),
                input: build_task_input(node.triggers(), node.reads(), channels),
                packet_id: None,
                origin_task_id: None,
                cached_writes: Vec::new(),
            })
        })
        .collect()
}

/// Applies task writes to channels and returns the channels updated this step.
pub fn apply_writes(
    checkpoint: &mut loom_graph::memory::Checkpoint<serde_json::Value>,
    channels: &mut HashMap<ChannelName, BoxedChannel>,
    tasks: &[ExecutableTask],
    graph: &PregelGraph,
    next_version: impl Fn(Option<&str>) -> ChannelVersion,
) -> Vec<ChannelName> {
    let mut grouped: BTreeMap<ChannelName, Vec<ChannelValue>> = BTreeMap::new();
    let mut updated_channels = Vec::new();
    let mut pending_sends = Vec::new();
    let mut pending_writes = Vec::new();

    for task in tasks {
        for (channel, value) in &task.writes {
            match classify_reserved_write(channel) {
                Some(ReservedWrite::Tasks)
                | Some(ReservedWrite::Push)
                | Some(ReservedWrite::Scheduled) => {
                    if let Some(packet) = decode_send_packet(
                        value.clone(),
                        Some(task.prepared.id.clone()),
                        task.prepared.step,
                    ) {
                        push_unique_pending_send(
                            &mut pending_sends,
                            task.prepared.id.clone(),
                            packet,
                        );
                    } else {
                        push_unique_pending_write(
                            &mut pending_writes,
                            task.prepared.id.clone(),
                            channel.clone(),
                            value.clone(),
                        );
                    }
                }
                Some(ReservedWrite::NoWrites) => {}
                Some(_) => {
                    push_unique_pending_write(
                        &mut pending_writes,
                        task.prepared.id.clone(),
                        channel.clone(),
                        value.clone(),
                    );
                }
                None => {
                    grouped
                        .entry(channel.clone())
                        .or_default()
                        .push(value.clone());
                }
            }
        }
    }

    let current_max = checkpoint
        .channel_versions
        .values()
        .max()
        .map(std::string::String::as_str);
    let version = next_version(current_max);

    for (channel_name, values) in grouped {
        if let Some(channel) = channels.get_mut(&channel_name) {
            if channel.update(&values) {
                checkpoint
                    .channel_versions
                    .insert(channel_name.clone(), version.clone());
                updated_channels.push(channel_name);
            }
        }
    }

    for task in tasks {
        let node_channels: HashSet<&str> = graph
            .nodes
            .get(&task.prepared.node_name)
            .map(|n| {
                n.reads()
                    .iter()
                    .chain(n.triggers().iter())
                    .map(String::as_str)
                    .collect()
            })
            .unwrap_or_default();
        checkpoint
            .versions_seen
            .entry(task.prepared.node_name.clone())
            .or_default()
            .extend(
                updated_channels
                    .iter()
                    .filter(|ch| node_channels.contains(ch.as_str()))
                    .map(|ch| (ch.clone(), version.clone())),
            );
    }

    checkpoint.pending_sends = pending_sends;
    checkpoint.pending_writes = pending_writes;

    for channel in channels.values_mut() {
        channel.consume();
    }

    checkpoint.updated_channels = Some(updated_channels.clone());
    checkpoint.channel_values = snapshot_channels(channels);
    updated_channels
}

/// Marks all channels as finished (no longer available for scheduling).
pub fn finish_channels(channels: &mut HashMap<ChannelName, BoxedChannel>) {
    for channel in channels.values_mut() {
        channel.finish();
    }
}

/// Creates a snapshot of all available channels.
pub fn snapshot_channels(channels: &HashMap<ChannelName, BoxedChannel>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    let mut names = channels.keys().cloned().collect::<Vec<_>>();
    names.sort();
    for name in names {
        if let Some(channel) = channels.get(&name) {
            map.insert(name, channel.snapshot());
        }
    }
    serde_json::Value::Object(map)
}

/// Restores runtime channels from a checkpoint or from graph defaults.
pub fn restore_channels_from_checkpoint(
    checkpoint: &loom_graph::memory::Checkpoint<serde_json::Value>,
    graph: &PregelGraph,
) -> HashMap<ChannelName, BoxedChannel> {
    let mut channels = HashMap::new();
    for (name, spec) in &graph.channels {
        let mut channel = build_channel(spec);
        if let Some(value) = checkpoint.channel_values.get(name) {
            let _ = channel.update(std::slice::from_ref(value));
        }
        if name == TASKS_CHANNEL {
            let task_values = checkpoint
                .pending_sends
                .iter()
                .filter(|(_, channel_name, _)| channel_name == TASKS_CHANNEL)
                .map(|(_, _, value)| value.clone())
                .collect::<Vec<_>>();
            if !task_values.is_empty() {
                let _ = channel.update(&task_values);
            }
        }
        channels.insert(name.clone(), channel);
    }
    channels
}

/// Derives a stable task identifier from a namespace, node, step, and kind.
pub fn task_id_for(namespace: &str, node_name: &str, step: u64, kind: TaskKind) -> TaskId {
    let mut hasher = DefaultHasher::new();
    namespace.hash(&mut hasher);
    node_name.hash(&mut hasher);
    step.hash(&mut hasher);
    kind.hash(&mut hasher);
    format!("task-{step}-{:#x}", hasher.finish())
}

/// Derives a stable task-cache key for a prepared task, scoped to the
/// current thread and checkpoint namespace so that different runs never
/// share cached writes.
pub fn task_cache_key(task: &PreparedTask, config: &RunnableConfig) -> TaskCacheKey {
    let mut hasher = DefaultHasher::new();
    stable_hash_value(&task.input, &mut hasher);
    TaskCacheKey {
        node_name: task.node_name.clone(),
        step: task.step,
        input_hash: format!("{:#x}", hasher.finish()),
        kind: task.kind,
        thread_id: config.thread_id.clone(),
        checkpoint_ns: config.checkpoint_ns.clone(),
    }
}

fn build_task_input(
    triggers: &[ChannelName],
    reads: &[ChannelName],
    channels: &HashMap<ChannelName, BoxedChannel>,
) -> ChannelValue {
    let mut map = serde_json::Map::new();
    let mut names = triggers
        .iter()
        .chain(reads.iter())
        .cloned()
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    for name in names {
        if let Some(channel) = channels.get(&name) {
            map.insert(name, channel.snapshot());
        }
    }
    ChannelValue::Object(map)
}

fn stable_hash_value(value: &ChannelValue, hasher: &mut DefaultHasher) {
    match value {
        ChannelValue::Null => {
            0u8.hash(hasher);
        }
        ChannelValue::Bool(v) => {
            1u8.hash(hasher);
            v.hash(hasher);
        }
        ChannelValue::Number(v) => {
            2u8.hash(hasher);
            v.to_string().hash(hasher);
        }
        ChannelValue::String(v) => {
            3u8.hash(hasher);
            v.hash(hasher);
        }
        ChannelValue::Array(values) => {
            4u8.hash(hasher);
            values.len().hash(hasher);
            for value in values {
                stable_hash_value(value, hasher);
            }
        }
        ChannelValue::Object(map) => {
            5u8.hash(hasher);
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            for key in keys {
                key.hash(hasher);
                stable_hash_value(&map[key], hasher);
            }
        }
    }
}

fn build_push_input(payload: ChannelValue) -> ChannelValue {
    match payload {
        ChannelValue::Object(map) => ChannelValue::Object(map),
        other => {
            let mut map = serde_json::Map::new();
            map.insert("$payload".to_string(), other);
            ChannelValue::Object(map)
        }
    }
}

fn classify_reserved_write(channel: &str) -> Option<ReservedWrite> {
    [
        ReservedWrite::Error,
        ReservedWrite::Interrupt,
        ReservedWrite::Resume,
        ReservedWrite::Scheduled,
        ReservedWrite::Push,
        ReservedWrite::Return,
        ReservedWrite::NoWrites,
        ReservedWrite::Tasks,
    ]
    .into_iter()
    .find(|reserved| reserved.as_str() == channel)
}

fn decode_send_packet(
    value: ChannelValue,
    default_origin_task_id: Option<TaskId>,
    default_origin_step: u64,
) -> Option<SendPacket> {
    match serde_json::from_value::<SendPacket>(value.clone()) {
        Ok(packet) => Some(packet),
        Err(_) => {
            let object = value.as_object()?;
            let target = object.get("target")?.as_str()?.to_string();
            let payload = object.get("payload").cloned().unwrap_or(ChannelValue::Null);
            let packet_id = object
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("pkt-{}-{}", target, default_origin_step));
            Some(SendPacket::new(
                packet_id,
                target,
                payload,
                default_origin_task_id,
                default_origin_step,
            ))
        }
    }
}

fn push_unique_pending_send(
    pending_sends: &mut Vec<(TaskId, ChannelName, ChannelValue)>,
    task_id: TaskId,
    packet: SendPacket,
) {
    let value = serde_json::to_value(packet.clone()).expect("send packet serializes");
    if let Some(existing) = pending_sends
        .iter_mut()
        .find(|(_, channel, existing_value)| {
            if channel != TASKS_CHANNEL {
                return false;
            }
            decode_send_packet(existing_value.clone(), None, packet.origin_step)
                .map(|existing_packet| existing_packet.id == packet.id)
                .unwrap_or(false)
        })
    {
        *existing = (task_id, TASKS_CHANNEL.to_string(), value);
        return;
    }
    pending_sends.push((task_id, TASKS_CHANNEL.to_string(), value));
}

fn push_unique_pending_write(
    pending_writes: &mut Vec<(TaskId, ChannelName, ChannelValue)>,
    task_id: TaskId,
    channel: ChannelName,
    value: ChannelValue,
) {
    if !pending_write_is_singleton(channel.as_str()) {
        if pending_writes
            .iter()
            .any(|(existing_task_id, existing_channel, existing_value)| {
                existing_task_id == &task_id
                    && existing_channel == &channel
                    && existing_value == &value
            })
        {
            return;
        }
        pending_writes.push((task_id, channel, value));
        return;
    }
    if let Some(existing) =
        pending_writes
            .iter_mut()
            .find(|(existing_task_id, existing_channel, _)| {
                existing_task_id == &task_id && existing_channel == &channel
            })
    {
        *existing = (task_id, channel, value);
        return;
    }
    pending_writes.push((task_id, channel, value));
}

fn pending_write_is_singleton(channel: &str) -> bool {
    matches!(
        classify_reserved_write(channel),
        Some(
            ReservedWrite::Return
                | ReservedWrite::Error
                | ReservedWrite::Resume
                | ReservedWrite::NoWrites
        )
    )
}

fn prepare_pull_tasks(
    tasks_by_id: &mut BTreeMap<TaskId, PreparedTask>,
    channels: &HashMap<ChannelName, BoxedChannel>,
    graph: &PregelGraph,
    step: u64,
    updated: &std::collections::HashSet<&str>,
) {
    for (node_name, node) in &graph.nodes {
        let should_run = if step == 0 && updated.is_empty() {
            node.triggers()
                .iter()
                .any(|trigger| graph.input_channels.iter().any(|input| input == trigger))
        } else {
            node.triggers().iter().any(|trigger| {
                updated.contains(trigger.as_str())
                    && channels
                        .get(trigger.as_str())
                        .is_some_and(|ch| ch.is_available())
            })
        };

        if !should_run {
            continue;
        }

        let input = build_task_input(node.triggers(), node.reads(), channels);
        let task_id = task_id_for("pregel", node_name, step, TaskKind::Pull);
        tasks_by_id.insert(
            task_id.clone(),
            PreparedTask {
                id: task_id,
                kind: TaskKind::Pull,
                node_name: node_name.clone(),
                step,
                triggers: node.triggers().to_vec(),
                input,
                packet_id: None,
                origin_task_id: None,
                cached_writes: Vec::new(),
            },
        );
    }
}

fn prepare_push_tasks(
    tasks_by_id: &mut BTreeMap<TaskId, PreparedTask>,
    checkpoint: &loom_graph::memory::Checkpoint<serde_json::Value>,
    graph: &PregelGraph,
    step: u64,
) {
    for (_, channel_name, value) in &checkpoint.pending_sends {
        if channel_name != TASKS_CHANNEL {
            continue;
        }
        let Some(packet) = decode_send_packet(value.clone(), None, step.saturating_sub(1)) else {
            continue;
        };
        if !graph.nodes.contains_key(&packet.target) {
            continue;
        }
        let task_id = task_id_for(&packet.id, &packet.target, step, TaskKind::Push);
        tasks_by_id.insert(
            task_id.clone(),
            PreparedTask {
                id: task_id,
                kind: TaskKind::Push,
                node_name: packet.target.clone(),
                step,
                triggers: vec![TASKS_CHANNEL.to_string()],
                input: build_push_input(packet.payload.clone()),
                packet_id: Some(packet.id),
                origin_task_id: packet.origin_task_id,
                cached_writes: Vec::new(),
            },
        );
    }
}
