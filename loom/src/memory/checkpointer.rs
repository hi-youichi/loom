//! Checkpointer trait and checkpoint persistence errors.
//!
//! A [`Checkpointer`] persists and retrieves per-run checkpoints addressed by
//! `(thread_id, checkpoint_ns, checkpoint_id)`. Higher-level runtimes use it to
//! resume runs, inspect history, and fork execution from earlier snapshots.

use async_trait::async_trait;

use crate::memory::checkpoint::{Checkpoint, CheckpointListItem, CheckpointMetadata};
use crate::memory::config::RunnableConfig;

/// Error type for checkpoint operations.
#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    #[error("thread_id required")]
    ThreadIdRequired,
    #[error("serialization: {0}")]
    Serialization(String),
    #[error("storage: {0}")]
    Storage(String),
    #[error("not found: {0}")]
    NotFound(String),
}

/// Persists and retrieves checkpoints for one state type.
///
/// Implementations are expected to treat `thread_id` plus checkpoint namespace as
/// the primary partition key. When `config.checkpoint_id` is absent,
/// [`Self::get_tuple`] should return the latest checkpoint in that lineage.
#[async_trait]
pub trait Checkpointer<S>: Send + Sync
where
    S: Clone + Send + Sync + 'static,
{
    /// Persists a checkpoint for the selected run lineage.
    ///
    /// Returns the checkpoint id that was stored, which is usually
    /// `checkpoint.id` but may be backend-normalized if needed.
    async fn put(
        &self,
        config: &RunnableConfig,
        checkpoint: &Checkpoint<S>,
    ) -> Result<String, CheckpointError>;

    /// Loads one checkpoint plus its metadata.
    ///
    /// When `config.checkpoint_id` is set, implementations should resolve that
    /// specific checkpoint. Otherwise they should return the latest checkpoint in
    /// the selected thread and namespace.
    async fn get_tuple(
        &self,
        config: &RunnableConfig,
    ) -> Result<Option<(Checkpoint<S>, CheckpointMetadata)>, CheckpointError>;

    /// Lists checkpoint history metadata for the selected lineage.
    ///
    /// `before` and `after` are backend-defined cursors or checkpoint ids used
    /// for paging through time-travel history.
    async fn list(
        &self,
        config: &RunnableConfig,
        limit: Option<usize>,
        before: Option<&str>,
        after: Option<&str>,
    ) -> Result<Vec<CheckpointListItem>, CheckpointError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Scenario**: Display of each CheckpointError variant contains expected keywords.
    #[test]
    fn checkpoint_error_display_all_variants() {
        assert!(CheckpointError::ThreadIdRequired
            .to_string()
            .to_lowercase()
            .contains("thread"));
        assert!(CheckpointError::Serialization("err".into())
            .to_string()
            .to_lowercase()
            .contains("serialization"));
        assert!(CheckpointError::Storage("io".into())
            .to_string()
            .to_lowercase()
            .contains("storage"));
        assert!(CheckpointError::NotFound("id".into())
            .to_string()
            .to_lowercase()
            .contains("not found"));
    }
}
