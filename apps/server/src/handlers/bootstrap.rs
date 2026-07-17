//! Bootstrap handlers — read-only introspection endpoints the TUI
//! hits before users see the chat surface.
//!
//! Tasks: P0.2 (v2 routes under `/api/...`) + P0.3 (v1 routes under
//! `/...`). Both shapes are exposed so a single `loom-server` can serve
//! v1 and v2 TUI builds simultaneously.
//!
//! Spec mapping:
//! - `/config`, `/config/providers`, `/provider`, `/agent` — v1 schema
//!   (`packages/schema/src/.../*.ts`).
//! - `/api/config`, `/api/provider`, `/api/agent`, `/api/model`,
//!   `/api/command`, `/api/skill`, `/api/reference`, `/api/integration` —
//!   v2 schema (`protocols/http/{config,provider,agent,...}.md`).
//!
//! Two design choices worth flagging in the design doc:
//! 1. We return empty provider lists until a real provider registry
//!    ships — `modelgate.dev` rejects `openai/*` lookups, so prefer
//!    leaving the picker blank over feeding stale entries
//!    (`agent_runner.rs` falls back to `LOOM_MODEL`).
//! 2. We log every bootstrap call once per second per path at
//!    `debug` so the `Promise.all` blizzard isn't loud in `info`.

// Include the real provider-group handlers (provider.rs) as a private
// submodule. `mod.rs` does not yet register `pub mod provider;` — that
// wiring happens in W4 along with the routes.rs update. Until then these
// bootstrap wrappers keep the existing routes alive by delegating.
#[path = "provider.rs"]
mod provider;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::location::{location_response, LocationInfo, LocationQuery};
use crate::state::SharedState;

/// App name used to locate `~/<app>/config.toml`.
const CONFIG_APP_NAME: &str = "loom";

// ───────────────────────────── v2 routes ──────────────────────────────

/// `GET /api/location` — v2 spec defines this as the canonical
/// "where am I" route (replaces v1 `/project`). Returns the currently
/// active workspace + project info.
///
/// The `worktree`/`directory` fields here are **informational metadata**
/// only — they report where the server is operating. This server does NOT
/// manage a git worktree lifecycle (see the 501 worktree handlers in
/// `experimental.rs`); clients must not read `worktree` as evidence that
/// create/reset/delete worktree operations are available.
pub async fn get_api_location(
    State(state): State<SharedState>,
    Query(_loc): Query<LocationQuery>,
) -> Json<Value> {
    // Contract (location.ts:29-42): GET /api/location returns bare
    // Location.Info = { directory, workspaceID?, project: { id, directory } },
    // NOT wrapped in Location.response.
    Json(serde_json::to_value(LocationInfo::from_state(&state)).unwrap_or(Value::Null))
}

/// `PATCH /api/location` — TUI may switch the active workspace via this
/// route. We accept and persist the choice so subsequent `/api/event`
/// envelopes carry the new workspace id (task P0.4 envelope).
pub async fn patch_api_location(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    {
        let mut project = state.project.write();
        if let Some(wid) = body.get("workspaceID").and_then(|v| v.as_str()) {
            project.set_workspace(Some(wid.to_string()));
        }
        if let Some(dir) = body.get("directory").and_then(|v| v.as_str()) {
            project.directory = dir.to_string();
            project.worktree = dir.to_string();
        }
    }
    get_api_location(State(state), Query(LocationQuery::default())).await
}

/// `GET /api/config` — v2 global config (alias of v1 `/config`).
pub async fn get_api_config(State(state): State<SharedState>) -> Json<Value> {
    let cfg = state.config.read().clone();
    Json(serde_json::to_value(&cfg).unwrap_or(Value::Null))
}

/// `PATCH /api/config` — partial update of `AppState::config` (task P2.23).
pub async fn patch_api_config(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    {
        let mut cfg = state.config.write();
        if let Some(theme) = body.get("theme").and_then(|v| v.as_str()) {
            cfg.theme = Some(theme.to_string());
        }
        if let Some(model) = body.get("model").and_then(|v| v.as_str()) {
            cfg.model = Some(model.to_string());
        }
        if let Some(provider) = body.get("provider") {
            cfg.provider = Some(provider.clone());
        }
        if let Some(arr) = body.get("providers").and_then(|v| v.as_array()) {
            cfg.providers = arr.clone();
        }
        // Store any unrecognized keys under `extra`.
        let mut rest = body.as_object().cloned().unwrap_or_default();
        rest.remove("theme");
        rest.remove("model");
        rest.remove("provider");
        rest.remove("providers");
        if !rest.is_empty() {
            let mut acc = cfg.extra.as_object().cloned().unwrap_or_default();
            for (k, v) in rest {
                acc.insert(k, v);
            }
            cfg.extra = Value::Object(acc);
        }
    }
    get_api_config(State(state)).await
}

/// `GET /api/provider` — v2 list of `Provider.Info` sourced from
/// `~/.loom/config.toml` `[[providers]]`. Delegates to [`provider::list`].
pub async fn get_api_providers(
    State(state): State<SharedState>,
    Query(loc): Query<LocationQuery>,
) -> Json<Value> {
    provider::list(State(state), Query(loc)).await
}

/// `GET /api/provider/{id}` — v2 single-provider lookup from config.
/// Delegates to [`provider::get`]; returns `404 ProviderNotFoundError`
/// when the id is absent.
pub async fn get_api_provider(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(loc): Query<LocationQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    provider::get(State(state), Path(id), Query(loc)).await
}

/// `GET /api/agent` — list of available agent configurations.
///
/// Returns the `Location.response` envelope with `Agent.Info[]`. Per
/// schema/agent.ts, `Agent.Info` has NO `name` field — `id` is the identifier.
pub async fn get_api_agents(
    State(state): State<SharedState>,
    Query(_loc): Query<LocationQuery>,
) -> Json<Value> {
    let agents = json!([{
        "id": "loom",
        "mode": "primary",
        "hidden": false,
        "permissions": [],
        "description": "Loom ReAct agent",
        "request": { "headers": {}, "body": {} },
    }]);
    location_response(&state, agents)
}

/// `GET /api/model` — list of available model configurations.
///
/// Returns the `Location.response` envelope with `Model.Info[]`. Models are
/// read from `~/.loom/config.toml` providers (declared `[[providers.models]]`
/// and default `model` fields) — real data, not stubs.
pub async fn get_api_models(
    State(state): State<SharedState>,
    Query(_loc): Query<LocationQuery>,
) -> Json<Value> {
    let models = build_model_infos();
    location_response(&state, Value::Array(models))
}

/// `GET /api/command` — v2 command registry.
///
/// Returns the `Location.response` envelope with `Command.Info[]`. Per
/// schema/command.ts, `Command.Info` uses `name` (not `id`) as identifier.
pub async fn get_api_commands(
    State(state): State<SharedState>,
    Query(_loc): Query<LocationQuery>,
) -> Json<Value> {
    let commands = json!([
        { "name": "init", "template": "init", "description": "Initialize a project" },
        { "name": "review", "template": "review", "description": "Review current changes" },
    ]);
    location_response(&state, commands)
}

/// `GET /api/skill` — v2 skill registry.
///
/// Returns the `Location.response` envelope with `Skill.Info[]`.
pub async fn get_api_skills(
    State(state): State<SharedState>,
    Query(_loc): Query<LocationQuery>,
) -> Json<Value> {
    location_response(&state, json!([]))
}

/// `GET /api/reference` — v2 reference links.
///
/// Returns the `Location.response` envelope with `Reference.Info[]`.
pub async fn get_api_references(
    State(state): State<SharedState>,
    Query(_loc): Query<LocationQuery>,
) -> Json<Value> {
    location_response(&state, json!([]))
}

/// `GET /api/integration` — v2 integration providers.
///
/// Returns the `Location.response` envelope with `Integration.Info[]`.
pub async fn get_api_integrations(
    State(state): State<SharedState>,
    Query(_loc): Query<LocationQuery>,
) -> Json<Value> {
    location_response(&state, json!([]))
}

/// `GET /api/path` — environment PATH + cwd list.
pub async fn get_api_path() -> Json<Value> {
    let cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let app_data = std::env::var("APPDATA").unwrap_or_else(|_| home.clone());
    let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| home.clone());
    Json(json!({
        "cwd": cwd,
        "root": cwd,
        "worktree": cwd,
        "directory": cwd,
        "home": home,
        "state": local.clone(),
        "config": app_data,
        "cache": local,
    }))
}

/// `GET /api/fs/list` — directory listing. MVP returns the cwd entry only.
pub async fn get_api_fs_list(
    axum::extract::Query(query): axum::extract::Query<VfsListQuery>,
) -> Json<Value> {
    let path = query.path.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    });
    Json(json!({
        "path": path,
        "entries": [],
    }))
}

// fs.find (POST) removed in W1 — the contract uses GET; see routes.rs TODO.

#[derive(serde::Deserialize, Default)]
pub struct VfsListQuery {
    #[serde(default)]
    pub path: Option<String>,
}

/// Mask an API key for display: first 3 chars + "***", or empty string if absent.
fn mask_key(key: &Option<String>) -> String {
    match key {
        Some(k) if k.len() >= 3 => format!("{}***", &k[..3]),
        Some(_) => "***".to_string(),
        None => String::new(),
    }
}

/// Build a models object map `{ modelId: { id, providerID, name } }` from a provider def.
fn build_models_map(def: &config::ProviderDef) -> Value {
    let mut map = serde_json::Map::new();
    if !def.models.is_empty() {
        for m in &def.models {
            map.insert(m.id.clone(), json!({
                "id": m.id,
                "providerID": def.name,
                "name": m.id,
            }));
        }
    } else if let Some(ref model) = def.model {
        map.insert(model.clone(), json!({
            "id": model,
            "providerID": def.name,
            "name": model,
        }));
    }
    Value::Object(map)
}

/// v1 `/config/providers` response in OpenCode-compatible format.
/// Returns `{ providers: [...], default: { providerId: modelId } }`.
pub async fn get_config_providers() -> Json<Value> {
    let cfg = match config::load_full_config(CONFIG_APP_NAME) {
        Ok(c) => c,
        Err(_) => return Json(json!({"providers": [], "default": {}})),
    };

    let providers: Vec<Value> = cfg.providers.iter().map(|def| {
        json!({
            "id": def.name,
            "name": def.name,
            "source": "api",
            "env": ["OPENAI_API_KEY"],
            "key": mask_key(&def.api_key),
            "options": {},
            "models": build_models_map(def),
        })
    }).collect();

    let mut default = serde_json::Map::new();
    if let Some(ref name) = cfg.default_provider {
        if let Some(def) = cfg.providers.iter().find(|p| &p.name == name) {
            let model_id = if !def.models.is_empty() {
                def.models[0].id.clone()
            } else if let Some(ref m) = def.model {
                m.clone()
            } else {
                String::new()
            };
            if !model_id.is_empty() {
                default.insert(name.clone(), Value::String(model_id));
            }
        }
    }

    Json(json!({"providers": providers, "default": Value::Object(default)}))
}

/// v1 `/provider` response. Uses OpenCode-compatible provider objects.
pub async fn get_provider_list() -> Json<Value> {
    let cfg = match config::load_full_config(CONFIG_APP_NAME) {
        Ok(c) => c,
        Err(_) => return Json(json!({"all": [], "default": {}, "connected": []})),
    };

    let build_entry = |def: &config::ProviderDef| {
        json!({
            "id": def.name,
            "name": def.name,
            "source": "api",
            "env": ["OPENAI_API_KEY"],
            "key": mask_key(&def.api_key),
            "options": {},
            "models": build_models_map(def),
        })
    };

    let all: Vec<Value> = cfg.providers.iter().map(build_entry).collect();
    let connected: Vec<Value> = cfg.providers.iter()
        .filter(|p| p.api_key.is_some())
        .map(build_entry)
        .collect();

    let mut default = serde_json::Map::new();
    if let Some(ref name) = cfg.default_provider {
        if let Some(def) = cfg.providers.iter().find(|p| &p.name == name) {
            let model_id = if !def.models.is_empty() {
                def.models[0].id.clone()
            } else if let Some(ref m) = def.model {
                m.clone()
            } else {
                String::new()
            };
            if !model_id.is_empty() {
                default.insert(name.clone(), Value::String(model_id));
            }
        }
    }

    Json(json!({"all": all, "default": Value::Object(default), "connected": connected}))
}

/// v1 `/agent` response. Agent names are used directly in prompt bodies.
pub async fn get_agent_list() -> Json<Value> {
    Json(json!([{
        "id": "build",
        "name": "build",
        "description": "Loom ReAct coding agent",
        "mode": "primary",
        "hidden": false,
        "permission": [],
        "permissions": [],
        "options": {},
        "request": {"headers": {}, "body": {}},
    }]))
}

/// Build the v1 `/project` envelope. The `worktree` field is
/// informational directory metadata only — no git worktree lifecycle is
/// managed here (see the 501 worktree handlers in `experimental.rs`).
fn project_value(state: &SharedState) -> Value {
    let project = state.project.read();
    let now = chrono::Utc::now().timestamp_millis();
    json!({
        "id": project.id,
        "worktree": project.worktree,
        "directory": project.directory,
        "vcs": project.vcs,
        "time": {"created": now, "updated": now},
        "sandboxes": [],
    })
}

/// v1 `/project` returns a list; `/project/current` returns one project.
pub async fn get_project_list(State(state): State<SharedState>) -> Json<Value> {
    Json(json!([project_value(&state)]))
}

pub async fn get_project_current(State(state): State<SharedState>) -> Json<Value> {
    Json(project_value(&state))
}

/// Current v2 SDK aliases for `/api/app/*` bootstrap calls.
pub async fn get_v2_agent_list() -> Json<Value> {
    Json(json!([{
        "id": "build",
        "name": "build",
        "description": "Loom ReAct coding agent",
        "mode": "primary",
        "hidden": false,
        "systemPrompt": "",
        "permission": [],
        "options": {},
    }]))
}

pub async fn get_v2_model_list() -> Json<Value> {
    Json(json!([]))
}

pub async fn get_v2_provider_list() -> Json<Value> {
    Json(json!([]))
}

/// Load `~/.loom/config.toml` and build `Model.Info[]` from declared
/// `[[providers.models]]` specs and each provider's default `model` field.
/// Returns an empty `Vec` when config is missing or has no models.
fn build_model_infos() -> Vec<Value> {
    let cfg = match config::load_full_config(CONFIG_APP_NAME) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut result = Vec::new();
    for def in &cfg.providers {
        if !def.models.is_empty() {
            for m in &def.models {
                result.push(json!({
                    "id": m.id,
                    "providerID": def.name,
                    "name": m.id,
                    "api": { "id": m.id, "type": "native", "settings": {} },
                    "capabilities": { "tools": true, "input": [], "output": [] },
                    "request": { "headers": {}, "body": {} },
                    "variants": [],
                    "time": { "released": 0 },
                    "cost": [],
                    "status": "active",
                    "enabled": true,
                    "limit": { "context": m.context_limit, "output": m.output_limit },
                }));
            }
        } else if let Some(ref model) = def.model {
            // Provider has a default model but no declared specs — emit a
            // Model.Info.empty() for it.
            result.push(json!({
                "id": model,
                "providerID": def.name,
                "name": model,
                "api": { "id": model, "type": "native", "settings": {} },
                "capabilities": { "tools": true, "input": [], "output": [] },
                "request": { "headers": {}, "body": {} },
                "variants": [],
                "time": { "released": 0 },
                "cost": [],
                "status": "active",
                "enabled": true,
                "limit": { "context": 0, "output": 0 },
            }));
        }
    }
    result
}

/// v1 `/command` response.
pub async fn get_command_list() -> Json<Value> {
    Json(json!([
        {"name": "init", "description": "Initialize a project", "template": "init", "hints": []},
        {"name": "review", "description": "Review current changes", "template": "review", "hints": []}
    ]))
}
