//! Memory implementations for Loom: checkpointing and long-term store.
//!
//! This crate provides concrete implementations of the checkpointing and store
//! traits defined in `loom-graph::memory`.
//!
//! ## Checkpointer Implementations
//!
//! | Type             | Persistence | Use case                    | Feature  |
//! |------------------|-------------|-----------------------------|----------|
//! | [`MemorySaver`]  | In-memory   | Dev, tests                  | —        |
//! | [`SqliteSaver`]  | SQLite file | Single-node, production     | —        |
//!
//! ## Store Implementations
//!
//! | Type             | Persistence | Search                      | Feature  |
//! |------------------|-------------|-----------------------------|----------|
//! | [`InMemoryStore`] | In-memory   | String filter (key/value)   | —        |
//! | [`SqliteStore`]   | SQLite file | String filter               | —        |
//! | [`SqliteVecStore`] | SQLite file | Vector similarity (semantic) | —      |
//! | [`InMemoryVectorStore`] | In-memory | Vector similarity (semantic) | —   |
//! | [`LanceStore`]    | LanceDB     | Vector similarity (semantic)| `lance`  |

// Re-export core types from loom-graph
pub use loom_graph::memory::{
    Checkpoint, CheckpointError, CheckpointListItem, CheckpointMetadata, CheckpointSource,
    CheckpointTuple, CheckpointUserMeta, Checkpointer, ChannelVersions, KernelMetadata,
    PendingWrite, RunnableConfig, Store, StoreError, StoreOp,
    Item, SearchItem, Namespace,
    FilterOp, ListNamespacesOptions, MatchCondition, NamespaceMatchType,
    SearchOptions, StoreOpResult, StoreSearchHit,
    writes_idx_map, uuid6, uuid6_with_params, Uuid6,
    CHECKPOINT_VERSION, ERROR, INTERRUPT, RESUME, SCHEDULED,
};

// Concrete implementations
mod embedder;
mod in_memory_store;
mod in_memory_vector_store;
mod memory_saver;
mod openai_embedder;
mod serializer;
#[cfg(feature = "lance")]
mod lance_store;
mod sqlite_saver;
mod sqlite_store;
pub mod sqlite_util;
mod sqlite_vec_store;

pub use in_memory_store::InMemoryStore;
pub use memory_saver::MemorySaver;
pub use serializer::{
    JsonSerializer, Serializer, TypedData, TypedSerializer, TYPE_BYTES, TYPE_JSON, TYPE_NULL,
};

pub use embedder::Embedder;
pub use in_memory_vector_store::InMemoryVectorStore;
#[cfg(feature = "lance")]
pub use lance_store::LanceStore;
pub use openai_embedder::OpenAIEmbedder;
pub use sqlite_saver::SqliteSaver;
pub use sqlite_store::SqliteStore;
pub use sqlite_vec_store::SqliteVecStore;

/// Returns the default SQLite memory database path.
///
/// This is the path used by helpers that need a conventional on-disk location
/// for the built-in memory store implementations.
pub fn default_memory_db_path() -> std::path::PathBuf {
    sqlite_util::default_memory_db_path()
}

/// Global mutex for tests that modify environment variables.
#[cfg(test)]
pub fn env_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}
