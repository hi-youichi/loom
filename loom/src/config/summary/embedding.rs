//! Embedding config block for run config summary.
//!
//! Implements [`ConfigSection`](super::ConfigSection). Used by CLI or other callers
//! to build the "Embedding" line. Does not include api_key.

use super::ConfigSection;

/// Embedding configuration summary: model and api_base only (no api_key).
///
/// Built from RunConfig embedding fields (effective values, e.g. default model/base).
pub struct EmbeddingConfigSummary {
    /// Embedding model name, e.g. `text-embedding-3-small`.
    pub model: String,
    /// API base URL used for embeddings.
    pub api_base: String,
}

impl ConfigSection for EmbeddingConfigSummary {
    fn section_name(&self) -> &str {
        "Embedding"
    }

    fn entries(&self) -> Vec<(&'static str, String)> {
        vec![
            ("model", self.model.clone()),
            ("api_base", self.api_base.clone()),
        ]
    }
}
