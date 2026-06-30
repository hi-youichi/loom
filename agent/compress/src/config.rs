//! Compaction configuration for context window management.

use model_spec_core::ModelTier;

/// Configuration for context compression.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    pub auto: bool,
    pub prune: bool,
    pub max_context_tokens: u32,
    pub reserve_tokens: u32,
    pub prune_keep_tokens: u32,
    pub prune_minimum: Option<u32>,
    pub compact_keep_recent: usize,
    pub compact_tier: ModelTier,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            auto: true, prune: true,
            max_context_tokens: 128_000, reserve_tokens: 4096,
            prune_keep_tokens: 40_000, prune_minimum: Some(20_000),
            compact_keep_recent: 20, compact_tier: ModelTier::Light,
        }
    }
}

impl CompactionConfig {
    pub fn with_max_context_tokens(max_context_tokens: u32) -> Self {
        Self { max_context_tokens, ..Self::default() }
    }
}
