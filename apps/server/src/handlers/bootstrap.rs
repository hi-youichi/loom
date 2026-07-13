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

use axum::{extract::Path, extract::State, http::StatusCode, Json};
use serde_json::{json, Value};

use crate::state::SharedState;

// ───────────────────────────── v2 routes ──────────────────────────────

/// `GET /api/location` — v2 spec defines this as the canonical
/// "where am I" route (replaces v1 `/project`). Returns the currently
/// active workspace + project info.
pub async fn get_api_location(State(state): State<SharedState>) -> Json<Value> {
    let project = state.project.read();
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let config = std::env::var("APPDATA").unwrap_or_else(|_| home.clone());
    let cache = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| home.clone());
    Json(json!({
        // Current v2 `LocationInfo` fields.
        "cwd": project.directory,
        "userDataDir": home,
        "configDir": config,
        "cacheDir": cache,
        "stateDir": cache,
        // Rollout-v2 compatibility fields retained for older generated SDKs.
        "id": project.id,
        "worktree": project.worktree,
        "directory": project.directory,
        "vcs": project.vcs,
        "workspaceID": project.workspace_id,
    }))
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
    get_api_location(State(state)).await
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

/// `GET /api/provider` — v2 list of `Provider.Info`. Empty for now.
pub async fn get_api_providers() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `GET /api/provider/{id}` — v2 single-provider lookup. Returns 404.
pub async fn get_api_provider(Path(_id): Path<String>) -> (StatusCode, Json<Value>) {
    (StatusCode::NOT_FOUND, Json(json!({ "data": null })))
}

/// `GET /api/agent` — list of available agent configurations.
pub async fn get_api_agents() -> Json<Value> {
    Json(json!({
        "data": [{
            "id": "loom",
            "name": "Loom",
            "mode": "primary",
            "hidden": false,
            "permissions": [],
            "description": "Loom ReAct agent",
            "request": { "headers": {}, "body": {} },
        }]
    }))
}

/// `GET /api/model` — list of available model configurations.
pub async fn get_api_models() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `GET /api/command` — v2 command registry.
pub async fn get_api_commands() -> Json<Value> {
    Json(json!({ "data": [
        { "id": "init", "description": "Initialize a project (placeholder)", "template": "init" },
        { "id": "review", "description": "Review current changes", "template": "review" },
    ] }))
}

/// `GET /api/skill` — v2 skill registry.
pub async fn get_api_skills() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `GET /api/reference` — v2 reference links.
pub async fn get_api_references() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `GET /api/integration` — v2 integration providers.
pub async fn get_api_integrations() -> Json<Value> {
    Json(json!({ "data": [] }))
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

/// `POST /api/fs/find` — name-based file search. Not implemented.
pub async fn post_api_fs_find(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!({ "matches": [] }))
}

#[derive(serde::Deserialize, Default)]
pub struct VfsListQuery {
    #[serde(default)]
    pub path: Option<String>,
}

/// v1 `/config/providers` response. The SDK expects provider metadata and a
/// provider-to-default-model map, not the v2 `{data: ...}` envelope.
pub async fn get_config_providers() -> Json<Value> {
    Json(json!({"providers": [], "default": {}}))
}

/// v1 `/provider` response.
pub async fn get_provider_list() -> Json<Value> {
    Json(json!({"all": [], "default": {}, "connected": []}))
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

/// v1 `/command` response.
pub async fn get_command_list() -> Json<Value> {
    Json(json!([
        {"name": "init", "description": "Initialize a project", "template": "init", "hints": []},
        {"name": "review", "description": "Review current changes", "template": "review", "hints": []}
    ]))
}
