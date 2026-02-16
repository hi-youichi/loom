//! Unit tests for Embedder trait.
//!
//! Verifies async embed works when called from within tokio runtime (regression for
//! runtime-within-runtime panic). Uses MockEmbedder, no API key required.

mod init_logging;

use async_trait::async_trait;
use graphweave::memory::{Embedder, StoreError};
use std::sync::Arc;

/// Mock embedder: deterministic vector per text for testing.
struct MockEmbedder {
    dimension: usize,
}

impl MockEmbedder {
    fn new(dimension: usize) -> Self {
        Self { dimension }
    }

    fn text_to_vec(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0f32; self.dimension];
        for (i, b) in text.bytes().enumerate() {
            v[i % self.dimension] += b as f32 / 256.0;
        }
        v
    }
}

#[async_trait]
impl Embedder for MockEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, StoreError> {
        Ok(texts.iter().map(|t| self.text_to_vec(t)).collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

/// **Scenario**: Async embed can be awaited from within tokio runtime without panic.
/// Regression test for "runtime within runtime" when embed was sync with block_on.
#[tokio::test]
async fn embedder_async_embed_works_within_tokio() {
    let embedder = Arc::new(MockEmbedder::new(8));
    let vectors = embedder
        .embed(&["hello", "world"])
        .await
        .expect("embed should succeed");

    assert_eq!(vectors.len(), 2);
    assert_eq!(vectors[0].len(), 8);
    assert_eq!(vectors[1].len(), 8);
    assert_ne!(vectors[0], vectors[1]);
}
