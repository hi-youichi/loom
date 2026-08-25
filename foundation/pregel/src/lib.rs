//! Pregel-style BSP graph runtime for anureo.
//!
//! This crate provides the low-level Pregel graph execution engine:
//! channels, task scheduling, checkpointing integration, subgraph support,
//! and the main PregelRuntime loop.

pub mod algo;
pub mod cache;
pub mod channel;
pub mod config;
pub mod graph_view;
pub mod loop_state;
pub mod node;
pub mod replay;
pub mod runner;
pub mod runtime;
pub mod state;
pub mod subgraph;
pub mod types;
pub mod validate;

// Re-export key types for convenience
pub use algo::{apply_writes, finish_channels, ExecutableTask, PreparedTask, TaskOutcome};
pub use config::{PregelConfig, PregelDurability};
pub use node::{PregelGraph, PregelNode, PregelNodeContext, PregelNodeInput, PregelNodeOutput};
pub use runtime::PregelRuntime;
pub use types::{
    ChannelName, ChannelValue, InterruptRecord, LoopStatus, ManagedValues, NodeName,
    PregelScratchpad, ReservedWrite, ResumeMap, TaskId, TaskKind, TASKS_CHANNEL,
};
