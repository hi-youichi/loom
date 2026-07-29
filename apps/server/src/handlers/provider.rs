//! Provider group handlers — opencode v2 `Provider.Info` from Loom config
//! (group-provider.ts, schema-provider.ts).
//!
//! Implements `GET /api/provider` (list) and `GET /api/provider/:providerID`
//! (get). Both endpoints are location-scoped (they accept [`LocationQuery`]
//! and wrap responses via [`location_response`]) per the contract's
//! `query: LocationQuery` / `success: Location.response(...)`.
//!
//! Provider data is sourced from `~/.loom/config.toml` `[[providers]]` — real
//! providers and their declared models, not stubs. Credentials (`api_key`)
//! are NEVER returned; the `request` field uses empty maps and the `api`
//! `settings` carry only non-secret configuration.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Map, Value};

use crate::location::{location_response, LocationQuery};
use crate::state::SharedState;

/// App name used to locate `~/<app>/config.toml`. Matches the rest of
/// loom-server and the `config` crate's XDG loader.
const CONFIG_APP_NAME: &str = "loom";

/// `GET /api/provider` — list active providers as `Provider.Info[]`.
///
/// Returns the `Location.response` envelope `{ location: Location.Info, data:
/// Provider.Info[] }`. Providers are read from `~/.loom/config.toml`
/// `[[providers]]`; a missing or empty config yields an empty list (graceful
/// degradation — never a stub).
pub async fn list(
    State(state): State<SharedState>,
    Query(_loc): Query<LocationQuery>,
) -> Json<Value> {
    let infos = build_provider_infos();
    location_response(&state, Value::Array(infos))
}

/// `GET /api/provider/:providerID` — single provider lookup.
///
/// Returns `404 ProviderNotFoundError` when the id is not present in the
/// config; otherwise the `Location.response` envelope with `Provider.Info`.
pub async fn get(
    State(state): State<SharedState>,
    Path(provider_id): Path<String>,
    Query(_loc): Query<LocationQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let infos = build_provider_infos();
    match infos
        .into_iter()
        .find(|p| p.get("id").and_then(Value::as_str) == Some(provider_id.as_str()))
    {
        Some(info) => Ok(location_response(&state, info)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "ProviderNotFoundError",
                "message": format!("provider '{provider_id}' not found in ~/.loom/config.toml"),
            })),
        )),
    }
}

// ──────────────────── config → Provider.Info mapping ────────────────────

/// Load `~/.loom/config.toml` and map every `[[providers]]` entry to a
/// `Provider.Info` JSON value (schema-provider.ts).
///
/// Returns an empty `Vec` when config is missing or has no providers.
fn build_provider_infos() -> Vec<Value> {
    config::load_full_config(CONFIG_APP_NAME)
        .map(|cfg| cfg.providers.iter().map(provider_info).collect())
        .unwrap_or_default()
}

/// Map one `ProviderDef` (config.toml) to the `Provider.Info` shape.
///
/// - `id` / `name` ← provider name (matches `Provider.Info.empty(id)` which
///   sets `name = id`).
/// - `api` ← `Native` type: `url` from `base_url` (optional), `settings`
///   carrying non-secret config (providerType, default model, temperature,
///   fetchModels, declared `[[providers.models]]`). `api_key` is deliberately
///   excluded — credentials are never serialized into a response.
/// - `request` ← empty headers/body maps (no auth material exposed).
/// - `integrationID` / `disabled` ← omitted (optional fields; not in config).
fn provider_info(def: &config::ProviderDef) -> Value {
    let mut settings = Map::new();
    if let Some(ref pt) = def.provider_type {
        settings.insert("providerType".into(), json!(pt));
    }
    if let Some(ref model) = def.model {
        settings.insert("model".into(), json!(model));
    }
    if let Some(temp) = def.temperature {
        if temp.is_finite() {
            settings.insert("temperature".into(), json!(temp));
        }
    }
    if let Some(fetch) = def.fetch_models {
        settings.insert("fetchModels".into(), json!(fetch));
    }
    if !def.models.is_empty() {
        let models: Vec<Value> = def
            .models
            .iter()
            .map(|m| {
                json!({
                    "id": m.id,
                    "contextLimit": m.context_limit,
                    "outputLimit": m.output_limit,
                    "reasoningEfforts": m.reasoning_efforts,
                })
            })
            .collect();
        settings.insert("models".into(), json!(models));
    }

    let mut api = Map::new();
    api.insert("type".into(), json!("native"));
    if let Some(ref url) = def.base_url {
        api.insert("url".into(), json!(url));
    }
    api.insert("settings".into(), Value::Object(settings));

    json!({
        "id": def.name,
        "name": def.name,
        "api": Value::Object(api),
        "request": { "headers": {}, "body": {} },
    })
}
