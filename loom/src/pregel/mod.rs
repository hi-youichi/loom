//! Pregel-style runtime building blocks.
//!
//! This module exposes Loom's low-level graph runtime inspired by Pregel and
//! LangGraph's bulk-synchronous execution model. A graph is described in terms
//! of named channels and nodes:
//!
//! - [`PregelGraph`] stores the static graph definition.
//! - [`PregelNode`] declares which channels a node subscribes to and reads.
//! - [`PregelRuntime`] validates the graph, drives execution, and optionally
//!   persists checkpoints or reuses cached task writes.
//! - [`PregelLoop`] is the mutable per-run state machine used internally while a
//!   run advances through bulk-synchronous barriers.
//!
//! The public API is split into a few layers:
//!
//! - Definition and validation: [`PregelGraph`], [`ChannelSpec`],
//!   [`PregelRuntime::validate`].
//! - Introspection: [`PregelGraphView`], [`PregelRuntime::get_graph`],
//!   [`PregelRuntime::get_subgraphs`].
//! - Execution: [`PregelRuntime::invoke`], [`PregelRuntime::stream`],
//!   [`PregelRuntime::invoke_subgraph`].
//! - Persistence and replay: [`PregelRuntime::get_state`],
//!   [`PregelRuntime::get_state_history`], [`PregelRuntime::replay`].
//!
//! This runtime is still evolving, but the exported types are intended to make
//! the execution model inspectable and testable before it becomes the default
//! engine behind higher-level Loom APIs.

mod algo;
mod cache;
mod channel;
mod config;
mod graph_view;
mod loop_state;
mod node;
mod replay;
mod runner;
mod runtime;
mod state;
mod subgraph;
mod types;
mod validate;

pub use algo::{
    apply_writes, finish_channels, normalize_pending_sends, normalize_pending_writes,
    prepare_next_tasks, restore_channels_from_checkpoint, snapshot_channels, task_id_for,
    ExecutableTask, PreparedTask, TaskOutcome,
};
pub use cache::{CachedTaskWrites, InMemoryPregelTaskCache, PregelTaskCache, TaskCacheKey};
pub use channel::{
    BinaryAggregateChannel, BoxedChannel, Channel, ChannelKind, ChannelSpec, LastValueChannel,
    ReducerFn, TopicChannel,
};
pub use config::{PregelConfig, PregelDurability};
pub use graph_view::{
    PregelGraphChannelView, PregelGraphEdgeKind, PregelGraphEdgeView, PregelGraphNodeView,
    PregelGraphView, PregelNamedGraphView,
};
pub use loop_state::{InterruptState, PregelLoop};
pub use node::{PregelGraph, PregelNode, PregelNodeContext, PregelNodeInput, PregelNodeOutput};
pub use replay::{ReplayMode, ReplayRequest, ReplayResult};
pub use runner::PregelRunner;
pub use runtime::{PregelRuntime, PregelStream};
pub use state::{BulkStateUpdateRequest, PregelStateSnapshot, StateUpdateRequest};
pub use subgraph::{
    CheckpointNamespace, PregelSubgraph, PregelSubgraphEntry, SubgraphInvocation, SubgraphResult,
};
pub use types::{
    ChannelName, ChannelValue, ChannelVersion, InterruptRecord, LoopStatus, ManagedValues,
    NodeName, PendingWrite, PregelScratchpad, ReservedWrite, ResumeMap, SendPacket, TaskId,
    TaskKind, TASKS_CHANNEL,
};
