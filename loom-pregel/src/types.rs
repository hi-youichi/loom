//! Core Pregel runtime types.

/// Name of a channel in the Pregel runtime.
pub type ChannelName = String;

/// Opaque node identifier.
pub type NodeName = String;

/// Opaque task identifier.
pub type TaskId = String;

/// Runtime channel payload value.
pub type ChannelValue = serde_json::Value;

/// Monotonic channel version value.
pub type ChannelVersion = String;

/// Pending write record persisted alongside checkpoints.
pub type PendingWrite = (TaskId, ChannelName, ChannelValue);

/// Runtime-managed values that are injected into node execution but are not normal channels.
pub type ManagedValues = std::collections::HashMap<String, ChannelValue>;

/// Reserved task mailbox channel used for push-style scheduling.
pub const TASKS_CHANNEL: &str = "__tasks__";

/// Reserved internal write kinds handled by the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReservedWrite {
    Error,
    Interrupt,
    Resume,
    Scheduled,
    Push,
    Return,
    NoWrites,
    Tasks,
}

impl ReservedWrite {
    /// Returns the persisted channel name for this reserved write.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "__error__",
            Self::Interrupt => "__interrupt__",
            Self::Resume => "__resume__",
            Self::Scheduled => "__scheduled__",
            Self::Push => "__push__",
            Self::Return => "__return__",
            Self::NoWrites => "__no_writes__",
            Self::Tasks => TASKS_CHANNEL,
        }
    }
}

/// Runtime loop status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoopStatus {
    Running,
    Done,
    InterruptedBefore,
    InterruptedAfter,
    Cancelled,
    Failed,
    OutOfSteps,
}

/// Origin of a prepared task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKind {
    Pull,
    Push,
}

/// Packet persisted in the task mailbox for push-style scheduling.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SendPacket {
    pub id: String,
    pub target: NodeName,
    pub payload: ChannelValue,
    pub origin_task_id: Option<TaskId>,
    pub origin_step: u64,
}

impl SendPacket {
    /// Creates a new send packet.
    pub fn new(
        id: impl Into<String>,
        target: impl Into<String>,
        payload: ChannelValue,
        origin_task_id: Option<TaskId>,
        origin_step: u64,
    ) -> Self {
        Self {
            id: id.into(),
            target: target.into(),
            payload,
            origin_task_id,
            origin_step,
        }
    }
}

/// Persisted interrupt metadata for checkpoint-backed resume.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InterruptRecord {
    pub interrupt_id: String,
    pub namespace: String,
    pub task_id: TaskId,
    pub node_name: NodeName,
    pub step: u64,
    pub value: ChannelValue,
}

/// Resume payloads keyed by interrupt id or namespace.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ResumeMap {
    pub values_by_namespace: std::collections::HashMap<String, ChannelValue>,
    pub values_by_interrupt_id: std::collections::HashMap<String, ChannelValue>,
}

/// Task-local scratchpad used for interrupt resume and ephemeral state.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct PregelScratchpad {
    pub task_id: TaskId,
    pub resume_value: Option<ChannelValue>,
    pub interrupt_counter: u32,
    pub local_state: std::collections::HashMap<String, ChannelValue>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reserved_write_as_str() {
        assert_eq!(ReservedWrite::Error.as_str(), "__error__");
        assert_eq!(ReservedWrite::Interrupt.as_str(), "__interrupt__");
        assert_eq!(ReservedWrite::Resume.as_str(), "__resume__");
        assert_eq!(ReservedWrite::Scheduled.as_str(), "__scheduled__");
        assert_eq!(ReservedWrite::Push.as_str(), "__push__");
        assert_eq!(ReservedWrite::Return.as_str(), "__return__");
        assert_eq!(ReservedWrite::NoWrites.as_str(), "__no_writes__");
        assert_eq!(ReservedWrite::Tasks.as_str(), "__tasks__");
    }

    #[test]
    fn test_send_packet_new() {
        let packet = SendPacket::new(
            "test-id",
            "target-node",
            serde_json::json!("payload"),
            Some("origin-task".to_string()),
            5,
        );
        assert_eq!(packet.id, "test-id");
        assert_eq!(packet.target, "target-node");
        assert_eq!(packet.payload, serde_json::json!("payload"));
        assert_eq!(packet.origin_task_id, Some("origin-task".to_string()));
        assert_eq!(packet.origin_step, 5);
    }

    #[test]
    fn test_send_packet_new_with_conversions() {
        let packet = SendPacket::new(
            "pkt-123".to_string(),
            "target",
            serde_json::json!(null),
            None,
            0,
        );
        assert_eq!(packet.id, "pkt-123");
        assert_eq!(packet.target, "target");
        assert!(packet.origin_task_id.is_none());
        assert_eq!(packet.origin_step, 0);
    }

    #[test]
    fn test_interrupt_record_fields() {
        let record = InterruptRecord {
            interrupt_id: "int-1".to_string(),
            namespace: "ns-1".to_string(),
            task_id: "task-1".to_string(),
            node_name: "node-1".to_string(),
            step: 10,
            value: serde_json::json!("test-value"),
        };
        assert_eq!(record.interrupt_id, "int-1");
        assert_eq!(record.namespace, "ns-1");
        assert_eq!(record.task_id, "task-1");
        assert_eq!(record.node_name, "node-1");
        assert_eq!(record.step, 10);
        assert_eq!(record.value, serde_json::json!("test-value"));
    }

    #[test]
    fn test_interrupt_record_equality() {
        let record1 = InterruptRecord {
            interrupt_id: "int-1".to_string(),
            namespace: "ns-1".to_string(),
            task_id: "task-1".to_string(),
            node_name: "node-1".to_string(),
            step: 10,
            value: serde_json::json!("test-value"),
        };
        let record2 = InterruptRecord {
            interrupt_id: "int-1".to_string(),
            namespace: "ns-1".to_string(),
            task_id: "task-1".to_string(),
            node_name: "node-1".to_string(),
            step: 10,
            value: serde_json::json!("test-value"),
        };
        assert_eq!(record1, record2);
    }

    #[test]
    fn test_resume_map_default() {
        let map = ResumeMap::default();
        assert!(map.values_by_namespace.is_empty());
        assert!(map.values_by_interrupt_id.is_empty());
    }

    #[test]
    fn test_resume_map_equality() {
        let mut map1 = ResumeMap::default();
        map1.values_by_namespace.insert("ns-1".to_string(), serde_json::json!("value1"));
        map1.values_by_interrupt_id.insert("int-1".to_string(), serde_json::json!("value2"));

        let mut map2 = ResumeMap::default();
        map2.values_by_namespace.insert("ns-1".to_string(), serde_json::json!("value1"));
        map2.values_by_interrupt_id.insert("int-1".to_string(), serde_json::json!("value2"));

        assert_eq!(map1, map2);
    }

    #[test]
    fn test_pregel_scratchpad_default() {
        let scratchpad = PregelScratchpad::default();
        assert!(scratchpad.task_id.is_empty());
        assert!(scratchpad.resume_value.is_none());
        assert_eq!(scratchpad.interrupt_counter, 0);
        assert!(scratchpad.local_state.is_empty());
    }

    #[test]
    fn test_pregel_scratchpad_with_values() {
        let mut local_state = std::collections::HashMap::new();
        local_state.insert("key".to_string(), serde_json::json!("value"));

        let scratchpad = PregelScratchpad {
            task_id: "task-1".to_string(),
            resume_value: Some(serde_json::json!("resume")),
            interrupt_counter: 5,
            local_state,
        };
        assert_eq!(scratchpad.task_id, "task-1");
        assert_eq!(scratchpad.resume_value, Some(serde_json::json!("resume")));
        assert_eq!(scratchpad.interrupt_counter, 5);
        assert_eq!(scratchpad.local_state.get("key"), Some(&serde_json::json!("value")));
    }

    #[test]
    fn test_loop_status_variants() {
        assert_eq!(LoopStatus::Running, LoopStatus::Running);
        assert_eq!(LoopStatus::Done, LoopStatus::Done);
        assert_eq!(LoopStatus::InterruptedBefore, LoopStatus::InterruptedBefore);
        assert_eq!(LoopStatus::InterruptedAfter, LoopStatus::InterruptedAfter);
        assert_eq!(LoopStatus::Cancelled, LoopStatus::Cancelled);
        assert_eq!(LoopStatus::Failed, LoopStatus::Failed);
        assert_eq!(LoopStatus::OutOfSteps, LoopStatus::OutOfSteps);

        assert_ne!(LoopStatus::Running, LoopStatus::Done);
        assert_ne!(LoopStatus::Done, LoopStatus::Failed);
    }

    #[test]
    fn test_loop_status_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(LoopStatus::Running);
        set.insert(LoopStatus::Done);
        set.insert(LoopStatus::Running);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_task_kind_variants() {
        assert_eq!(TaskKind::Pull, TaskKind::Pull);
        assert_eq!(TaskKind::Push, TaskKind::Push);
        assert_ne!(TaskKind::Pull, TaskKind::Push);
    }

    #[test]
    fn test_task_kind_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(TaskKind::Pull);
        set.insert(TaskKind::Push);
        set.insert(TaskKind::Pull);
        assert_eq!(set.len(), 2);
    }
}
