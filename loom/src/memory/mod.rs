//! # Memory: Checkpointing and Long-term Store
//!
//! [Checkpointer] + [Store] for persistence.
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
//! [`RunnableConfig`] is passed to `CompiledStateGraph::invoke`. When using a checkpointer:
//! - `thread_id`: Required. Identifies the conversation/thread.
//! - `checkpoint_id`: Optional. Load a specific checkpoint (time-travel / branch).
//! - `checkpoint_ns`: Optional namespace for subgraphs.
//! - `user_id`: Used by Store for multi-tenant isolation.
//!
//! ## Checkpointer Implementations
//!
//! | Type         | Persistence | Use case                    | Feature  |
//! |--------------|-------------|-----------------------------|----------|
//! | [`MemorySaver`]  | In-memory   | Dev, tests                  | —        |
//! | [`SqliteSaver`]  | SQLite file | Single-node, production     | — |
//!
//! Use with [`StateGraph::compile_with_checkpointer`](crate::graph::StateGraph::compile_with_checkpointer).
//! [`JsonSerializer`] is required for `SqliteSaver` (state must be `Serialize + DeserializeOwned`).
//!
//! ## Store Implementations
//!
//! | Type             | Persistence | Search                      | Feature  |
//! |------------------|-------------|-----------------------------|----------|
//! | [`InMemoryStore`] | In-memory   | String filter (key/value)   | —        |
//! | [`SqliteStore`]   | SQLite file | String filter               | — |
//! | [`SqliteVecStore`] | SQLite file | Vector similarity (semantic) | — |
//! | [`LanceStore`]      | LanceDB     | Vector similarity (semantic)| `lance`  |
//! | [`InMemoryVectorStore`] | In-memory | Vector similarity (semantic) | — |
//!
//! `SqliteVecStore`, `LanceStore`, and `InMemoryVectorStore` require an `Embedder` for vector indexing; search with `query` uses semantic similarity.

mod checkpoint;
mod checkpointer;
mod config;
mod embedder;
mod in_memory_store;
mod in_memory_vector_store;
mod memory_saver;
mod openai_embedder;
mod serializer;
mod store;
mod uuid6;

#[cfg(feature = "lance")]
mod lance_store;
mod sqlite_saver;
mod sqlite_store;
mod sqlite_vec_store;

pub use checkpoint::{
    writes_idx_map, ChannelVersions, Checkpoint, CheckpointListItem, CheckpointMetadata,
    CheckpointSource, CheckpointTuple, PendingWrite, CHECKPOINT_VERSION, ERROR, INTERRUPT, RESUME,
    SCHEDULED,
};
pub use checkpointer::{CheckpointError, Checkpointer};
pub use config::RunnableConfig;
pub use in_memory_store::InMemoryStore;
pub use memory_saver::MemorySaver;
pub use serializer::{
    JsonSerializer, Serializer, TypedData, TypedSerializer, TYPE_BYTES, TYPE_JSON, TYPE_NULL,
};
pub use store::{
    FilterOp, Item, ListNamespacesOptions, MatchCondition, Namespace, NamespaceMatchType,
    SearchItem, SearchOptions, Store, StoreError, StoreOp, StoreOpResult, StoreSearchHit,
};
pub use uuid6::{uuid6, uuid6_with_params, Uuid6};

pub use embedder::Embedder;
pub use in_memory_vector_store::InMemoryVectorStore;
#[cfg(feature = "lance")]
pub use lance_store::LanceStore;
pub use openai_embedder::OpenAIEmbedder;
pub use sqlite_saver::SqliteSaver;
pub use sqlite_store::SqliteStore;
pub use sqlite_vec_store::SqliteVecStore;
