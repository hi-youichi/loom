use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;

use super::ModelResolver;
use crate::models_dev::yaml_provider::{load_yaml_plugins, YamlPluginFile};
use crate::Model;

/// A resolver backed by YAML plugin files.
///
/// YAML files in `~/.anureo/providers/*.yaml` define custom providers
/// that completely replace models.dev data for the given provider ID.
pub struct PluginModelResolver {
    /// provider_id → (provider_type, models)
    plugins: HashMap<String, PluginData>,
}

struct PluginData {
    provider_type: Option<String>,
    models: HashMap<String, Model>,
}

impl PluginModelResolver {
    /// Load plugins from a directory and create a resolver.
    pub fn load(dir: &std::path::Path) -> Self {
        let plugins = load_yaml_plugins(dir);
        let mut map: HashMap<String, PluginData> = HashMap::new();

        for plugin in plugins {
            let provider_type = plugin.provider.r#type.clone();
            let models = plugin
                .models
                .into_iter()
                .filter_map(|(id, def)| {
                    let model = def.into_model(&id)?;
                    Some((id, model))
                })
                .collect();

            map.insert(
                plugin.provider.id.clone(),
                PluginData {
                    provider_type,
                    models,
                },
            );
        }

        Self { plugins: map }
    }

    /// Create from pre-parsed YAML plugin files.
    pub fn from_plugins(plugins: Vec<YamlPluginFile>) -> Self {
        let mut map: HashMap<String, PluginData> = HashMap::new();

        for plugin in plugins {
            let provider_type = plugin.provider.r#type.clone();
            let models = plugin
                .models
                .into_iter()
                .filter_map(|(id, def)| {
                    let model = def.into_model(&id)?;
                    Some((id, model))
                })
                .collect();

            map.insert(
                plugin.provider.id.clone(),
                PluginData {
                    provider_type,
                    models,
                },
            );
        }

        Self { plugins: map }
    }

    /// Check if a provider is registered as a plugin.
    pub fn has_provider(&self, provider_id: &str) -> bool {
        self.plugins.contains_key(provider_id)
    }

    /// Get the provider type override for a provider, if any.
    pub fn provider_type(&self, provider_id: &str) -> Option<&str> {
        self.plugins
            .get(provider_id)
            .and_then(|p| p.provider_type.as_deref())
    }

    /// Return the list of provider IDs that have plugins.
    pub fn provider_ids(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }
}

#[async_trait]
impl ModelResolver for PluginModelResolver {
    async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<Model> {
        self.plugins
            .get(provider_id)
            .and_then(|data| data.models.get(model_id).cloned())
    }
}

/// Convenience function: load plugins from the default providers directory.
pub fn load_default_plugins() -> PluginModelResolver {
    let dir = dirs_default_providers_dir();
    PluginModelResolver::load(&dir)
}

/// Resolve the default providers directory (~/.anureo/providers/).
pub fn default_providers_dir() -> std::path::PathBuf {
    dirs_default_providers_dir()
}

fn dirs_default_providers_dir() -> PathBuf {
    anureo_home::anureo_home().join("providers")
}
