//! Compaction configuration for context window management.
//!
//! Controls when and how to prune tool results and compact conversation history.

use model_spec_core::spec::ModelTier;

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
    /// Model tier to use for compaction (defaults to Light).
    pub compact_tier: ModelTier,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            auto: true,
            prune: true,
            max_context_tokens: 128_000,
            reserve_tokens: 4096,
            prune_keep_tokens: 40_000,
            prune_minimum: Some(20_000),
            compact_keep_recent: 20,
            compact_tier: ModelTier::Light,
        }
    }
}

impl CompactionConfig {
    /// Create a config with a specific `max_context_tokens` (e.g., from models.dev).
    ///
    /// Other fields use defaults from `CompactionConfig::default()`.
    pub fn with_max_context_tokens(max_context_tokens: u32) -> Self {
        Self {
            max_context_tokens,
            ..Self::default()
        }
    }

    pub fn with_compact_tier(mut self, tier: ModelTier) -> Self {
        self.compact_tier = tier;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_enables_auto_and_prune() {
        let c = CompactionConfig::default();
        assert!(c.auto);
        assert!(c.prune);
        assert_eq!(c.max_context_tokens, 128_000);
        assert_eq!(c.reserve_tokens, 4096);
        assert_eq!(c.prune_keep_tokens, 40_000);
        assert_eq!(c.prune_minimum, Some(20_000));
        assert_eq!(c.compact_keep_recent, 20);
        assert_eq!(c.compact_tier, ModelTier::Light);
    }

    #[test]
    fn with_max_context_tokens_uses_defaults_for_other_fields() {
        let c = CompactionConfig::with_max_context_tokens(200_000);
        assert!(c.auto);
        assert!(c.prune);
        assert_eq!(c.max_context_tokens, 200_000);
        assert_eq!(c.reserve_tokens, 4096);
        assert_eq!(c.prune_keep_tokens, 40_000);
        assert_eq!(c.prune_minimum, Some(20_000));
        assert_eq!(c.compact_keep_recent, 20);
        assert_eq!(c.compact_tier, ModelTier::Light);
    }

    #[test]
    fn with_compact_tier_overrides_default() {
        let c = CompactionConfig::default().with_compact_tier(ModelTier::Standard);
        assert_eq!(c.compact_tier, ModelTier::Standard);
    }
}
