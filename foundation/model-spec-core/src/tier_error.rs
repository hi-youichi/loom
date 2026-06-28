//! Tier resolution and model registry error types.

use thiserror::Error;

/// Tier resolution and model registry errors.
#[derive(Debug, Error)]
pub(crate) enum TierError {
    #[error("{0}")]
    Execution(String),
}

impl TierError {
    pub(crate) fn execution(msg: impl Into<String>) -> Self {
        Self::Execution(msg.into())
    }
}
