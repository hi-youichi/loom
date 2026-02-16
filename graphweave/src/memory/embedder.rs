//! Embedder trait for LanceStore. Used to produce vectors from text for put and search.
//!
//! Implementations can wrap OpenAI, HuggingFace, or mock embedders for tests.

use async_trait::async_trait;

use crate::memory::store::StoreError;

/// Produces fixed-size float vectors from text. Used by [`crate::memory::LanceStore`]
/// for embedding value text on put and query text on search.
///
/// Implementations must be `Send + Sync` for use with async Store methods.
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Embeds each text into a vector of dimension [`Embedder::dimension`].
    /// Returns one vector per input text in the same order.
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, StoreError>;

    /// Vector dimension returned by [`Embedder::embed`].
    fn dimension(&self) -> usize;
}
