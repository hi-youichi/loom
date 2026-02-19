//! Model specification: context limit, output limit, and optional cache limits.

use serde::{Deserialize, Serialize};

/// Model token limit specification.
///
/// Used by context compression to determine when to prune or compact messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSpec {
    /// Context (input) token limit.
    pub context_limit: u32,
    /// Output token limit.
    pub output_limit: u32,
    /// Optional cache read token limit (e.g., for models with prompt caching).
    #[serde(default)]
    pub cache_read: Option<u32>,
    /// Optional cache write token limit.
    #[serde(default)]
    pub cache_write: Option<u32>,
}

impl ModelSpec {
    /// Create a new `ModelSpec` with required limits.
    pub fn new(context_limit: u32, output_limit: u32) -> Self {
        Self {
            context_limit,
            output_limit,
            cache_read: None,
            cache_write: None,
        }
    }

    /// Set optional cache read limit.
    pub fn with_cache_read(mut self, limit: u32) -> Self {
        self.cache_read = Some(limit);
        self
    }

    /// Set optional cache write limit.
    pub fn with_cache_write(mut self, limit: u32) -> Self {
        self.cache_write = Some(limit);
        self
    }
}
