//! # Memory: checkpointing and long-term store
//!
//! This module groups Loom's two persistence layers:
//!
//! - [`Checkpointer`] stores per-run snapshots for resume, replay, branching,
//!   and inspection.
//! - [`Store`] stores user or application data outside a single run, such as
//!   memories, preferences, and search indexes.
//!
//! ## Overview
//!
//! The memory module provides two distinct capabilities:
//!
//! 1. **Checkpointer** — Per-thread state snapshots for time-travel, branching, and resumable
//!    conversations. Keys checkpoints by `(thread_id, checkpoint_ns, checkpoint_id)`.
//! 2. **Store** — Cross-session key-value storage for long-term memory (preferences, facts, etc.).
//!    Isolated by [`Namespace`] (e.g. `[user_id, "memories"]`). Optional vector search via LanceDB.
//!
//! ## Config
//!
//! [`RunnableConfig`] is passed to graph and Pregel runtime execution methods.
//! When using a checkpointer:
//! - `thread_id`: Required. Identifies the conversation/thread.
//! - `checkpoint_id`: Optional. Load a specific checkpoint (time-travel / branch).
//! - `checkpoint_ns`: Optional namespace for subgraphs.
//! - `user_id`: Used by Store for multi-tenant isolation.
//!
//! ## Core Types
//!
//! ### Checkpointing
//! - [`Checkpoint`] - Graph execution snapshot with metadata
//! - [`Checkpointer`] - Trait for checkpoint persistence
//! - [`RunnableConfig`] - Execution configuration
//! - [`CheckpointMetadata`] - Checkpoint metadata
//!
//! ### Store
//! - [`Store`] - Trait for persistent key-value storage
//! - [`StoreOp`] - Batch store operations
//! - [`Item`] - Stored key-value items with metadata
//! - [`Namespace`] - Hierarchical namespace for store items
//!
//! ### Utilities
//! - [`uuid6`] - Generate UUID version 6 checkpoint IDs

pub mod checkpoint;
pub mod checkpointer;  
pub mod config;
pub mod store;
pub mod uuid6;

// Re-export core types
pub use checkpoint::{
    Checkpoint, CheckpointMetadata, CheckpointListItem, CheckpointTuple, CheckpointSource,
    CheckpointUserMeta, KernelMetadata, PendingWrite, ChannelVersions,
    CHECKPOINT_VERSION, ERROR, SCHEDULED, INTERRUPT, RESUME, writes_idx_map,
};
pub use checkpointer::{Checkpointer, CheckpointError};
pub use config::RunnableConfig;
pub use store::{
    Store, StoreOp, StoreError, StoreOpResult, StoreSearchHit,
    Item, SearchItem, Namespace,
    FilterOp, ListNamespacesOptions, MatchCondition, NamespaceMatchType, SearchOptions,
};
pub use uuid6::{uuid6, uuid6_with_params, Uuid6};