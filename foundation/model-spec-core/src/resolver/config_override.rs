//! Config override resolver: returns a fixed model when max_context_tokens is configured.

use async_trait::async_trait;

use super::ModelResolver;
use crate::{Model, ModelLimit};

/// Resolver that returns a fixed model based on explicit config.
///
/// Used as the highest-priority source in CompositeResolver when
/// `CompactionConfig.max_context_tokens` (and optionally output_limit) are set.
pub struct ConfigOverride {
    context_limit: u32,
    output_limit: Option<u32>,
}

impl ConfigOverride {
    /// Create with required context_limit. output_limit defaults to 64_000 if not set.
    pub fn new(context_limit: u32) -> Self {
        Self {
            context_limit,
            output_limit: None,
        }
    }

    /// Set output limit.
    pub fn with_output_limit(mut self, output_limit: u32) -> Self {
        self.output_limit = Some(output_limit);
        self
    }
}

#[async_trait]
impl ModelResolver for ConfigOverride {
    async fn resolve(&self, _provider_id: &str, model_id: &str) -> Option<Model> {
        Some(Model::minimal(
            model_id,
            ModelLimit::new(self.context_limit, self.output_limit.unwrap_or(64_000)),
        ))
    }
}
