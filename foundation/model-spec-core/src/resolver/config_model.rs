//! Resolver that reads model specs from `config.toml` `[[providers.models]]` declarations.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::ModelResolver;
use crate::{Model, ModelLimit};

/// A provider entry from config.toml with manually declared model specs.
pub struct ConfigProviderEntry {
    pub name: String,
    pub models: Vec<ConfigModelEntry>,
}

/// A single model spec entry from `[[providers.models]]` in config.toml.
pub struct ConfigModelEntry {
    pub id: String,
    pub context_limit: u32,
    pub output_limit: u32,
}

/// Resolver that serves model specs declared in `config.toml`.
///
/// Priority sits between `ConfigOverride` (global) and `CachedResolver<ModelsDevResolver>`.
pub struct ConfigModelResolver {
    specs: Arc<RwLock<HashMap<String, HashMap<String, Model>>>>,
}

impl ConfigModelResolver {
    pub fn new() -> Self {
        Self {
            specs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Build from a list of config provider entries.
    pub fn from_providers(providers: &[ConfigProviderEntry]) -> Self {
        let mut map: HashMap<String, HashMap<String, Model>> = HashMap::new();
        for p in providers {
            if p.models.is_empty() {
                continue;
            }
            let mut model_map = HashMap::new();
            for m in &p.models {
                model_map.insert(
                    m.id.clone(),
                    Model::minimal(&m.id, ModelLimit::new(m.context_limit, m.output_limit)),
                );
            }
            map.insert(p.name.clone(), model_map);
        }
        Self {
            specs: Arc::new(RwLock::new(map)),
        }
    }

    /// Insert or update a model at runtime.
    pub async fn upsert(&self, provider: &str, model_id: &str, model: Model) {
        let mut guard = self.specs.write().await;
        guard
            .entry(provider.to_string())
            .or_default()
            .insert(model_id.to_string(), model);
    }
}

impl Default for ConfigModelResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ModelResolver for ConfigModelResolver {
    async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<Model> {
        let guard = self.specs.read().await;
        guard.get(provider_id)?.get(model_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn from_providers_builds_lookup() {
        let providers = vec![ConfigProviderEntry {
            name: "zhipuai".into(),
            models: vec![
                ConfigModelEntry {
                    id: "glm-5.2".into(),
                    context_limit: 1_000_000,
                    output_limit: 131_072,
                },
                ConfigModelEntry {
                    id: "glm-4.6".into(),
                    context_limit: 204_800,
                    output_limit: 131_072,
                },
            ],
        }];
        let resolver = ConfigModelResolver::from_providers(&providers);

        let model = resolver.resolve("zhipuai", "glm-5.2").await.unwrap();
        assert_eq!(model.limit.context, 1_000_000);
        assert_eq!(model.limit.output, 131_072);

        let model2 = resolver.resolve("zhipuai", "glm-4.6").await.unwrap();
        assert_eq!(model2.limit.context, 204_800);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_returns_none_for_unknown() {
        let resolver = ConfigModelResolver::from_providers(&[]);
        assert!(resolver.resolve("zhipuai", "glm-5.2").await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn upsert_adds_new_model() {
        let resolver = ConfigModelResolver::new();
        resolver
            .upsert(
                "zhipuai",
                "glm-5.2",
                Model::minimal("glm-5.2", ModelLimit::new(1_000_000, 131_072)),
            )
            .await;

        let model = resolver.resolve("zhipuai", "glm-5.2").await.unwrap();
        assert_eq!(model.limit.context, 1_000_000);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn empty_provider_models_skipped() {
        let providers = vec![ConfigProviderEntry {
            name: "empty".into(),
            models: vec![],
        }];
        let resolver = ConfigModelResolver::from_providers(&providers);
        assert!(resolver.resolve("empty", "any").await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn config_model_has_tool_call_true() {
        let providers = vec![ConfigProviderEntry {
            name: "test".into(),
            models: vec![ConfigModelEntry {
                id: "m1".into(),
                context_limit: 128_000,
                output_limit: 4_096,
            }],
        }];
        let resolver = ConfigModelResolver::from_providers(&providers);
        let model = resolver.resolve("test", "m1").await.unwrap();
        assert!(model.tool_call);
    }
}
