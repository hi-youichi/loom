//! Shared configuration types for agent building.

use model_spec_core::spec::ModelTier;

/// Filter for builtin tools: whitelist (enabled) and blacklist (disabled).
#[derive(Clone, Debug, Default)]
pub struct BuiltinToolFilter {
    pub enabled: Option<Vec<String>>,
    pub disabled: Option<Vec<String>>,
}

impl BuiltinToolFilter {
    pub fn is_noop(&self) -> bool {
        self.enabled.as_ref().is_none_or(|v| v.is_empty())
            && self.disabled.as_ref().is_none_or(|v| v.is_empty())
    }

    pub fn is_allowed(&self, name: &str) -> bool {
        if let Some(ref en) = self.enabled {
            if !en.is_empty() && !en.iter().any(|e| e == name) {
                return false;
            }
        }
        if let Some(ref dis) = self.disabled {
            if dis.iter().any(|d| d == name) {
                return false;
            }
        }
        true
    }

    pub fn filter_names<'a>(&self, names: &'a [String]) -> Vec<&'a String> {
        names.iter().filter(|n| self.is_allowed(n.as_str())).collect()
    }
}

/// ToT-specific runner config.
#[derive(Clone, Debug)]
pub struct TotRunnerConfig {
    pub max_depth: u32,
    pub candidates_per_step: u32,
    pub research_quality_addon: bool,
}

impl Default for TotRunnerConfig {
    fn default() -> Self {
        Self { max_depth: 5, candidates_per_step: 3, research_quality_addon: false }
    }
}

/// GoT-specific runner config.
#[derive(Clone, Debug, Default)]
pub struct GotRunnerConfig {
    pub adaptive: bool,
    pub agot_llm_complexity: bool,
}

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
