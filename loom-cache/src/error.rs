//! Cache-related errors.

use thiserror::Error;

/// Errors that can occur when working with caches.
#[derive(Debug, Error)]
pub enum CacheError {
    /// General cache error.
    #[error("Cache error: {0}")]
    Other(String),
}
