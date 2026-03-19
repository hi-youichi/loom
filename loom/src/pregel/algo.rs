//! Core Pregel algorithms.

use std::collections::{hash_map::DefaultHasher, BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use crate::pregel::cache::TaskCacheKey;
use crate::pregel::channel::{build_channel, BoxedChannel};
use crate::pregel::node::PregelGraph;
use crate::pregel::types::{
    ChannelName, ChannelValue, ChannelVersion, ReservedWrite, SendPacket, TaskId, TaskKind,
    TASKS_CHANNEL,
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
    Success { task: ExecutableTask },
    Interrupted {
        task: ExecutableTask,
        interrupt: crate::graph::GraphInterrupt,
    },
    Cancelled { task: ExecutableTask },
    Failed {
        task: ExecutableTask,
        error: crate::error::AgentError,
    },
}

/// Prepares the next set of tasks from updated channels.
pub fn prepare_next_tasks(
    checkpoint: &crate::memory::Checkpoint<serde_json::Value>,
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

/// Applies task writes to channels and returns the channels updated this step.
pub fn apply_writes(
    checkpoint: &mut crate::memory::Checkpoint<serde_json::Value>,
    channels: &mut HashMap<ChannelName, BoxedChannel>,
    tasks: &[ExecutableTask],
    _graph: &PregelGraph,
    next_version: impl Fn(Option<&str>) -> ChannelVersion,
) -> Vec<ChannelName> {
    let mut grouped: BTreeMap<ChannelName, Vec<ChannelValue>> = BTreeMap::new();
    let mut updated_channels = Vec::new();
    let mut pending_sends = Vec::new();

    for task in tasks {
        for (channel, value) in &task.writes {
            match classify_reserved_write(channel) {
                Some(ReservedWrite::Tasks) | Some(ReservedWrite::Push) => {
                    if let Some(packet) =
                        decode_send_packet(value.clone(), Some(task.prepared.id.clone()), task.prepared.step)
                    {
                        pending_sends.push((
                            task.prepared.id.clone(),
                            TASKS_CHANNEL.to_string(),
                            serde_json::to_value(packet).expect("send packet serializes"),
                        ));
                    }
                }
                Some(_) => {}
                None => {
                    grouped.entry(channel.clone()).or_default().push(value.clone());
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
        checkpoint
            .versions_seen
            .entry(task.prepared.node_name.clone())
            .or_default()
            .extend(
                updated_channels
                    .iter()
                    .map(|ch| (ch.clone(), version.clone())),
            );
    }

    checkpoint.pending_sends = pending_sends;

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
    checkpoint: &crate::memory::Checkpoint<serde_json::Value>,
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

/// Derives a stable task-cache key for a prepared task.
pub fn task_cache_key(task: &PreparedTask) -> TaskCacheKey {
    let mut hasher = DefaultHasher::new();
    stable_hash_value(&task.input, &mut hasher);
    TaskCacheKey {
        node_name: task.node_name.clone(),
        step: task.step,
        input_hash: format!("{:#x}", hasher.finish()),
        kind: task.kind,
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
            let payload = object
                .get("payload")
                .cloned()
                .unwrap_or(ChannelValue::Null);
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
                        .map_or(false, |ch| ch.is_available())
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
    checkpoint: &crate::memory::Checkpoint<serde_json::Value>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pregel::channel::{Channel, ChannelKind, ChannelSpec, LastValueChannel};
    use crate::pregel::node::{PregelNode, PregelNodeContext, PregelNodeInput, PregelNodeOutput};
    use async_trait::async_trait;
    use std::sync::Arc;

    #[test]
    fn task_id_is_stable_for_same_input() {
        let a = task_id_for("pregel", "node", 1, TaskKind::Pull);
        let b = task_id_for("pregel", "node", 1, TaskKind::Pull);
        assert_eq!(a, b);
    }

    #[test]
    fn snapshot_channels_collects_values() {
        let mut channels: HashMap<ChannelName, BoxedChannel> = HashMap::new();
        let mut ch = LastValueChannel::new();
        ch.update(&[serde_json::json!(1)]);
        channels.insert("a".to_string(), Box::new(ch));

        let snapshot = snapshot_channels(&channels);
        assert_eq!(snapshot["a"], serde_json::json!(1));
    }

    #[test]
    fn restore_channels_rehydrates_from_checkpoint_values() {
        let mut graph = PregelGraph::new();
        graph.add_channel("a", ChannelSpec::new(ChannelKind::LastValue));
        let checkpoint = crate::memory::Checkpoint::from_state(
            serde_json::json!({"a": 7}),
            crate::memory::CheckpointSource::Loop,
            0,
        );

        let channels = restore_channels_from_checkpoint(&checkpoint, &graph);
        assert_eq!(channels.get("a").unwrap().snapshot(), serde_json::json!(7));
    }

    #[derive(Debug)]
    struct DummyNode {
        name: String,
        triggers: Vec<String>,
        reads: Vec<String>,
    }

    #[async_trait]
    impl PregelNode for DummyNode {
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
            _ctx: &PregelNodeContext,
        ) -> Result<PregelNodeOutput, crate::error::AgentError> {
            Ok(PregelNodeOutput::default())
        }
    }

    #[test]
    fn prepare_next_tasks_builds_push_task_from_pending_send() {
        let mut graph = PregelGraph::new();
        graph
            .add_channel(TASKS_CHANNEL, ChannelSpec::new(ChannelKind::Tasks))
            .add_node(Arc::new(DummyNode {
                name: "worker".to_string(),
                triggers: vec![TASKS_CHANNEL.to_string()],
                reads: vec!["payload".to_string()],
            }))
            .build_trigger_index();

        let mut checkpoint = crate::memory::Checkpoint::from_state(
            serde_json::json!({}),
            crate::memory::CheckpointSource::Loop,
            0,
        );
        checkpoint.pending_sends.push((
            "t0".to_string(),
            TASKS_CHANNEL.to_string(),
            serde_json::json!({
                "id": "pkt-1",
                "target": "worker",
                "payload": {"payload": "hello"},
                "origin_step": 0
            }),
        ));

        let tasks = prepare_next_tasks(&checkpoint, &HashMap::new(), &graph, 1, &[]);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].kind, TaskKind::Push);
        assert_eq!(tasks[0].node_name, "worker");
        assert_eq!(tasks[0].input["payload"], serde_json::json!("hello"));
    }

    #[test]
    fn apply_writes_persists_send_packets_as_pending_sends() {
        let graph = PregelGraph::new();
        let mut checkpoint = crate::memory::Checkpoint::from_state(
            serde_json::json!({}),
            crate::memory::CheckpointSource::Loop,
            0,
        );
        let mut channels = HashMap::new();
        let task = ExecutableTask {
            prepared: PreparedTask {
                id: "task-1".to_string(),
                kind: TaskKind::Pull,
                node_name: "n1".to_string(),
                step: 0,
                triggers: vec![],
                input: serde_json::json!({}),
                packet_id: None,
                origin_task_id: None,
                cached_writes: vec![],
            },
            writes: vec![(
                TASKS_CHANNEL.to_string(),
                serde_json::json!({"id": "pkt-1", "target": "worker", "payload": {"x": 1}}),
            )],
            attempt: 0,
        };

        let updated = apply_writes(&mut checkpoint, &mut channels, &[task], &graph, |current| {
            let next = current.and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) + 1;
            next.to_string()
        });

        assert!(updated.is_empty());
        assert_eq!(checkpoint.pending_sends.len(), 1);
        assert_eq!(checkpoint.pending_sends[0].1, TASKS_CHANNEL);
    }
}
