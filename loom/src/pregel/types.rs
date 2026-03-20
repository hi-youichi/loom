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
    fn reserved_write_names_are_stable() {
        assert_eq!(ReservedWrite::Error.as_str(), "__error__");
        assert_eq!(ReservedWrite::Interrupt.as_str(), "__interrupt__");
        assert_eq!(ReservedWrite::Resume.as_str(), "__resume__");
        assert_eq!(ReservedWrite::Scheduled.as_str(), "__scheduled__");
        assert_eq!(ReservedWrite::Tasks.as_str(), TASKS_CHANNEL);
    }
}
