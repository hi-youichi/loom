//! Composite resolver: chains multiple resolvers by priority.

use std::sync::Arc;

use async_trait::async_trait;

use super::ModelResolver;
use crate::Model;

/// Chains multiple resolvers; returns the first non-None result.
pub struct CompositeResolver {
    sources: Vec<Arc<dyn ModelResolver>>,
}

impl CompositeResolver {
    /// Create with the given sources, in priority order.
    pub fn new(sources: Vec<Arc<dyn ModelResolver>>) -> Self {
        Self { sources }
    }

    /// Add a source at the end of the chain.
    pub fn push(&mut self, source: Arc<dyn ModelResolver>) {
        self.sources.push(source);
    }
}

#[async_trait]
impl ModelResolver for CompositeResolver {
    async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<Model> {
        for source in &self.sources {
            if let Some(model) = source.resolve(provider_id, model_id).await {
                return Some(model);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::ConfigOverride;
    use crate::ModelLimit;

    #[tokio::test(flavor = "current_thread")]
    async fn config_override_takes_priority() {
        let config = Arc::new(ConfigOverride::new(100_000).with_output_limit(8_000));
        let empty = Arc::new(CompositeResolver::new(vec![]));
        let composite = CompositeResolver::new(vec![config, empty]);

        let model = composite.resolve("any", "model").await.unwrap();
        assert_eq!(model.limit.context, 100_000);
        assert_eq!(model.limit.output, 8_000);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn falls_through_to_next_source() {
        let fallback = Arc::new(ConfigOverride::new(50_000));
        let composite = CompositeResolver::new(vec![fallback]);

        let model = composite.resolve("x", "y").await.unwrap();
        assert_eq!(model.limit.context, 50_000);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn minimal_model_has_tool_call_true() {
        let model = Model::minimal("test", ModelLimit::new(128_000, 4_096));
        assert!(model.tool_call);
    }
}
