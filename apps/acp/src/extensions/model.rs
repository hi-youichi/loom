//! `model` extension domain — provider/model catalog for clients
//! (`_loomdesk.dev/model/*`).
//!
//! Serves the model picker from the Loom config (`[[providers]]` +
//! `[default]` in config.toml, resolved through the loom home directory/XDG). The default
//! provider additionally gets a live `/v1/models` listing when credentials
//! are available; every provider always exposes at least its declared or
//! default model.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const MODEL_CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct ModelCacheState {
    catalog: Option<Value>,
    refreshed_at: Option<Instant>,
    refreshing: bool,
}

static MODEL_CACHE: std::sync::LazyLock<tokio::sync::RwLock<ModelCacheState>> =
    std::sync::LazyLock::new(|| {
        tokio::sync::RwLock::new(ModelCacheState {
            catalog: None,
            refreshed_at: None,
            refreshing: false,
        })
    });

pub struct ModelHandler;

impl Default for ModelHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelHandler {
    pub fn new() -> Self {
        // Warm the cache in the background so the very first model/list call
        // after server start can already hit a populated cache.
        Self::kick_background_refresh(false);
        Self
    }

    /// Spawn a background refresh if one is not already running. When
    /// `only_if_empty` is set, skip when a catalog already exists (used by
    /// `new()` to avoid refresh storms across handler constructions).
    fn kick_background_refresh(only_if_empty: bool) {
        tokio::spawn(async move {
            let (should_refresh, already_running) = {
                let state = MODEL_CACHE.read().await;
                (
                    !(only_if_empty && state.catalog.is_some()),
                    state.refreshing,
                )
            };
            if !should_refresh || already_running {
                return;
            }
            {
                let mut state = MODEL_CACHE.write().await;
                if state.refreshing || (only_if_empty && state.catalog.is_some()) {
                    return;
                }
                state.refreshing = true;
            }
            let catalog = Self::build_catalog().await;
            let mut state = MODEL_CACHE.write().await;
            state.catalog = Some(catalog);
            state.refreshed_at = Some(Instant::now());
            state.refreshing = false;
        });
    }

    async fn refresh_blocking() -> Value {
        let catalog = Self::build_catalog().await;
        let mut state = MODEL_CACHE.write().await;
        state.catalog = Some(catalog.clone());
        state.refreshed_at = Some(Instant::now());
        state.refreshing = false;
        catalog
    }

    async fn catalog(&self) -> Value {
        let (cached, fresh) = {
            let state = MODEL_CACHE.read().await;
            (
                state.catalog.clone(),
                state
                    .refreshed_at
                    .map(|t| t.elapsed() < MODEL_CACHE_TTL)
                    .unwrap_or(false),
            )
        };
        match (cached, fresh) {
            (Some(catalog), true) => catalog,
            (Some(catalog), false) => {
                // Stale-while-revalidate: serve immediately, refresh in the
                // background. The live /v1/models fetches take seconds; the
                // catalog is advisory (model picker), never critical-path.
                Self::kick_background_refresh(false);
                catalog
            }
            (None, _) => {
                // Cold cache: this call blocks once; every later call hits
                // the cache. `new()` usually pre-warms so this is rare.
                Self::refresh_blocking().await
            }
        }
    }

    async fn build_catalog() -> Value {
        let default_model = config::default_model();
        let default_provider = config::default_provider_name();

        let provider_configs = config::load_provider_configs_from_xdg().unwrap_or_default();

        // Live model listings (parallel, bounded) for every provider with
        // credentials; declared models remain as the guaranteed floor.
        let live_lists = futures::future::join_all(provider_configs.iter().map(|p| async {
            let client = match (p.base_url.clone(), p.api_key.clone()) {
                (Some(base_url), Some(api_key)) => {
                    let entry_model = p
                        .declared_models
                        .first()
                        .cloned()
                        .unwrap_or_else(|| default_model.clone());
                    let entry = model_spec_core::registry::ModelEntry {
                        id: format!("{}/{}", p.name, entry_model),
                        name: entry_model,
                        provider: p.name.clone(),
                        base_url: Some(base_url),
                        api_key: Some(api_key),
                        provider_type: Some(
                            p.provider_type
                                .clone()
                                .unwrap_or_else(|| "openai_compat".to_string()),
                        ),
                        ..Default::default()
                    };
                    loom_llm::factory::create_llm_client(&entry, None).ok()
                }
                _ => None,
            };
            let listed = match client {
                Some(client) => client
                    .list_models()
                    .await
                    .map(|l| l.into_iter().map(|i| i.id).take(200).collect::<Vec<_>>())
                    .unwrap_or_default(),
                None => Vec::new(),
            };
            (p.name.clone(), listed)
        }))
        .await;
        let live_index: std::collections::HashMap<String, Vec<String>> =
            live_lists.into_iter().collect();

        let mut providers: Vec<Value> = Vec::new();
        let mut defaults = serde_json::Map::new();

        for p in &provider_configs {
            let mut models: Vec<String> = p.declared_models.clone();

            let is_default = default_provider.as_deref() == Some(p.name.as_str());
            if is_default && !models.iter().any(|m| m == &default_model) {
                models.insert(0, default_model.clone());
            }

            if let Some(live) = live_index.get(&p.name) {
                for id in live {
                    if !models.contains(id) {
                        models.push(id.clone());
                    }
                }
            }

            if is_default {
                defaults.insert(p.name.clone(), Value::String(default_model.clone()));
            }

            if models.is_empty() && !is_default {
                // Providers with no declared and no live models are noise in
                // the picker; skip them.
                continue;
            }

            providers.push(json!({
                "id": p.name,
                "name": p.name,
                "models": models.iter().map(|m| json!({ "id": m, "name": m })).collect::<Vec<_>>(),
            }));
        }

        // No config at all: synthesize a single openai-compatible entry from
        // env (proxy setups) with the built-in default model.
        if providers.is_empty() {
            let provider_id = "openai-compatible".to_string();
            defaults.insert(provider_id.clone(), Value::String(default_model.clone()));
            providers.push(json!({
                "id": provider_id,
                "name": provider_id,
                "models": [{ "id": default_model, "name": default_model }],
            }));
        }

        json!({
            "providers": providers,
            "default": Value::Object(defaults),
        })
    }
}

#[async_trait]
impl ExtensionHandler for ModelHandler {
    fn capabilities(&self) -> Value {
        json!({ "list": true })
    }

    async fn handle(
        &self,
        method: &str,
        _params: Value,
        _ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => {
                // `lastUsed` is stamped per-request (not part of the cached
                // catalog): it tracks `set_session_config_option("model")`
                // selections and changes far more often than the catalog.
                let mut catalog = self.catalog().await;
                if let Some(last) = crate::last_model::load() {
                    catalog["lastUsed"] = Value::String(last);
                }
                Ok(catalog)
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn catalog_has_provider_and_default_model() {
        let catalog = ModelHandler::new().catalog().await;
        let providers = catalog["providers"].as_array().expect("providers");
        assert!(!providers.is_empty());
        let defaults = catalog["default"].as_object().expect("default map");
        let (provider, model) = defaults.iter().next().expect("default entry");
        let entry = providers
            .iter()
            .find(|p| p["id"].as_str() == Some(provider.as_str()))
            .expect("default provider listed");
        let models = entry["models"].as_array().expect("models");
        let model_id = model.as_str().unwrap_or_default();
        assert!(models.iter().any(|m| m["id"].as_str() == Some(model_id)));
    }
}
