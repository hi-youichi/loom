//! # Checkpoint: checkpointing and long-term store abstractions
//!
//! This crate provides the persistence abstractions for Loom, aligned with
//! LangGraph's `langgraph-checkpoint` package:
//!
//! - [`Checkpointer`] stores per-run snapshots for resume, replay, branching,
//!   and inspection.
//! - [`Store`] stores user or application data outside a single run, such as
//!   memories, preferences, and search indexes.
//!
//! ## Overview
//!
//! The crate provides two distinct capabilities plus default in-memory
//! implementations:
//!
//! 1. **Checkpointer** — Per-thread state snapshots for time-travel, branching,
//!    and resumable conversations. Keys checkpoints by
//!    `(thread_id, checkpoint_ns, checkpoint_id)`.
//! 2. **Store** — Cross-session key-value storage for long-term memory
//!    (preferences, facts, etc.). Isolated by [`Namespace`].
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

mod in_memory_store;
mod memory_saver;
mod serializer;

// Re-export core types
pub use checkpoint::{
    writes_idx_map, ChannelVersions, Checkpoint, CheckpointListItem, CheckpointMetadata,
    CheckpointSource, CheckpointTuple, CheckpointUserMeta, KernelMetadata, PendingWrite,
    CHECKPOINT_VERSION, ERROR, INTERRUPT, RESUME, SCHEDULED,
};
pub use checkpointer::{CheckpointError, Checkpointer};
pub use config::RunnableConfig;
pub use store::{
    FilterOp, Item, ListNamespacesOptions, MatchCondition, Namespace, NamespaceMatchType,
    SearchItem, SearchOptions, Store, StoreError, StoreOp, StoreOpResult, StoreSearchHit,
};
pub use uuid6::{uuid6, uuid6_with_params, Uuid6};

// Default in-memory implementations
pub use in_memory_store::InMemoryStore;
pub use memory_saver::MemorySaver;
pub use serializer::{
    JsonSerializer, Serializer, TypedData, TypedSerializer, TYPE_BYTES, TYPE_JSON, TYPE_NULL,
};
