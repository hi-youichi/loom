//! Channel-related errors.

use thiserror::Error;

/// Errors that can occur when working with channels.
#[derive(Debug, Error)]
pub enum ChannelError {
    /// Invalid update operation.
    #[error("Invalid update: {0}")]
    InvalidUpdate(String),
}
