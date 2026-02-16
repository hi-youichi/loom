//! Composite resolver: chains multiple resolvers by priority.

use std::sync::Arc;

use async_trait::async_trait;

use super::resolver::ModelLimitResolver;
use super::spec::ModelSpec;

/// Chains multiple resolvers; returns the first non-None result.
pub struct CompositeResolver {
    sources: Vec<Arc<dyn ModelLimitResolver>>,
}

impl CompositeResolver {
    /// Create with the given sources, in priority order.
    pub fn new(sources: Vec<Arc<dyn ModelLimitResolver>>) -> Self {
        Self { sources }
    }

    /// Add a source at the end of the chain.
    pub fn push(&mut self, source: Arc<dyn ModelLimitResolver>) {
        self.sources.push(source);
    }
}

#[async_trait]
impl ModelLimitResolver for CompositeResolver {
    async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<ModelSpec> {
        for source in &self.sources {
            if let Some(spec) = source.resolve(provider_id, model_id).await {
                return Some(spec);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_spec::ConfigOverride;

    #[tokio::test]
    async fn config_override_takes_priority() {
        let config = Arc::new(ConfigOverride::new(100_000).with_output_limit(8_000));
        let empty = Arc::new(CompositeResolver::new(vec![]));
        let composite = CompositeResolver::new(vec![config, empty]);

        let spec = composite.resolve("any", "model").await.unwrap();
        assert_eq!(spec.context_limit, 100_000);
        assert_eq!(spec.output_limit, 8_000);
    }

    #[tokio::test]
    async fn falls_through_to_next_source() {
        let fallback = Arc::new(ConfigOverride::new(50_000));
        let composite = CompositeResolver::new(vec![fallback]);

        let spec = composite.resolve("x", "y").await.unwrap();
        assert_eq!(spec.context_limit, 50_000);
    }
}
