//! Compaction configuration for context window management.
//!
//! Controls when and how to prune tool results and compact conversation history.

/// Configuration for context compression: pruning and compaction.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Whether to auto-compact when context overflows (hybrid token estimate).
    pub auto: bool,
    /// Whether to prune old tool results beyond `prune_keep_tokens`.
    pub prune: bool,
    /// Maximum context size in tokens (overflow triggers compaction when auto is true).
    pub max_context_tokens: u32,
    /// Tokens to reserve for generation; overflow = current + reserve > max_context_tokens.
    pub reserve_tokens: u32,
    /// When pruning, keep at most this many tokens of tool results (from most recent).
    pub prune_keep_tokens: u32,
    /// Minimum tokens to prune in one pass; below this, no pruning is applied.
    pub prune_minimum: Option<u32>,
    /// When compacting, keep this many most recent messages; older ones are summarized.
    pub compact_keep_recent: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            auto: false,
            prune: false,
            max_context_tokens: 128_000,
            reserve_tokens: 4096,
            prune_keep_tokens: 40_000,
            prune_minimum: Some(20_000),
            compact_keep_recent: 20,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_disables_auto_and_prune() {
        let c = CompactionConfig::default();
        assert!(!c.auto);
        assert!(!c.prune);
        assert_eq!(c.max_context_tokens, 128_000);
        assert_eq!(c.reserve_tokens, 4096);
        assert_eq!(c.prune_keep_tokens, 40_000);
        assert_eq!(c.prune_minimum, Some(20_000));
        assert_eq!(c.compact_keep_recent, 20);
    }
}
