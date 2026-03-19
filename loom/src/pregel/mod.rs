//! Pregel-style runtime building blocks.
//!
//! This module is an early skeleton for a LangGraph Pregel-like runtime.
//! The current focus is type definitions and module boundaries; it is not
//! wired into Loom's existing execution paths yet.

mod algo;
mod cache;
mod channel;
mod config;
mod loop_state;
mod node;
mod replay;
mod runner;
mod runtime;
mod state;
mod subgraph;
mod types;

pub use algo::{
    apply_writes, finish_channels, prepare_next_tasks, restore_channels_from_checkpoint,
    snapshot_channels, task_id_for, ExecutableTask, PreparedTask, TaskOutcome,
};
pub use cache::{CachedTaskWrites, InMemoryPregelTaskCache, PregelTaskCache, TaskCacheKey};
pub use channel::{
    BinaryAggregateChannel, BoxedChannel, Channel, ChannelKind, ChannelSpec, LastValueChannel,
    ReducerFn, TopicChannel,
};
pub use config::{PregelConfig, PregelDurability};
pub use loop_state::{InterruptState, PregelLoop};
pub use node::{PregelGraph, PregelNode, PregelNodeContext, PregelNodeInput, PregelNodeOutput};
pub use replay::{ReplayMode, ReplayRequest, ReplayResult};
pub use runner::PregelRunner;
pub use runtime::{PregelRuntime, PregelStream};
pub use state::{BulkStateUpdateRequest, PregelStateSnapshot, StateUpdateRequest};
pub use subgraph::{CheckpointNamespace, SubgraphInvocation, SubgraphResult};
pub use types::{
    ChannelName, ChannelValue, ChannelVersion, InterruptRecord, LoopStatus, ManagedValues,
    NodeName, PendingWrite, PregelScratchpad, ReservedWrite, ResumeMap, SendPacket, TaskId,
    TaskKind, TASKS_CHANNEL,
};
