//! Vector search Store implementations for Loom.
//!
//! Provides embedders and vector-backed [`Store`] implementations for semantic
//! search over long-term memory.
//!
//! | Type                    | Persistence | Search                      | Feature |
//! |-------------------------|-------------|-----------------------------|---------|
//! | [`InMemoryVectorStore`] | In-memory   | Vector similarity (semantic)| —       |
//! | [`SqliteVecStore`]      | SQLite file | Vector similarity (sqlite-vec) | —    |
//! | [`LanceStore`]          | LanceDB     | Vector similarity (semantic)| `lance` |

mod embedder;
mod in_memory_vector_store;
mod openai_embedder;
mod sqlite_vec_store;
#[cfg(feature = "lance")]
mod lance_store;

pub use embedder::Embedder;
pub use in_memory_vector_store::InMemoryVectorStore;
pub use openai_embedder::OpenAIEmbedder;
pub use sqlite_vec_store::SqliteVecStore;
#[cfg(feature = "lance")]
pub use lance_store::LanceStore;
