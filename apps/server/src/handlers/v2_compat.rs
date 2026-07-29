//! Compatibility handlers for the current generated v2 SDK surface.
//!
//! The rollout design was written against an earlier v2 snapshot. These
//! handlers keep the newer resource-oriented URL aliases registered while the
//! in-memory MVP intentionally returns conservative empty/stub values for
//! subsystems that are not part of the session/agent critical path.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::state::{emit, make_session, SharedState};

pub async fn ok() -> Json<Value> {
    Json(json!({"ok": true}))
}

pub async fn true_value() -> Json<Value> {
    Json(json!(true))
}

pub async fn empty_list() -> Json<Value> {
    Json(json!([]))
}

pub async fn empty_object() -> Json<Value> {
    Json(json!({}))
}

pub async fn instance_start() -> Json<Value> {
    Json(json!({
        "instanceID": "loom-server",
        "version": env!("CARGO_PKG_VERSION"),
        "startedAt": chrono::Utc::now().timestamp_millis(),
        "running": true,
        "workspaces": [],
    }))
}

pub async fn instance_dispose() -> Json<Value> {
    Json(json!(true))
}

pub async fn set_workspace(State(state): State<SharedState>) -> Json<Value> {
    let mut project = state.project.write();
    let workspace_id = project.id.clone();
    project.set_workspace(Some(workspace_id));
    Json(json!({
        "cwd": project.directory,
        "userDataDir": project.directory,
        "configDir": project.directory,
        "cacheDir": project.directory,
        "stateDir": project.directory,
    }))
}

fn project_value(state: &SharedState) -> Value {
    let project = state.project.read();
    let now = chrono::Utc::now().timestamp_millis();
    json!({
        "id": project.id,
        "name": "loom",
        "root": project.directory,
        "createdAt": now,
        "updatedAt": now,
        "repositories": [],
        "metadata": {},
    })
}

pub async fn project_list(State(state): State<SharedState>) -> Json<Value> {
    Json(json!([project_value(&state)]))
}

pub async fn project_current(State(state): State<SharedState>) -> Json<Value> {
    Json(project_value(&state))
}

pub async fn project_update(State(state): State<SharedState>) -> Json<Value> {
    Json(project_value(&state))
}

pub async fn active_sessions(State(state): State<SharedState>) -> Json<Value> {
    let sessions = state
        .sessions
        .read()
        .values()
        .map(|session| json!({
            "id": session.id,
            "state": if crate::state::lookup_run(&state, &session.id).is_some() { "busy" } else { "idle" },
            "modelID": session.model.as_ref().map(|model| model.model_id.clone()).unwrap_or_default(),
            "providerID": session.model.as_ref().map(|model| model.provider_id.clone()).unwrap_or_default(),
            "agent": session.agent.clone().unwrap_or_else(|| "build".to_string()),
            "startedAt": session.time.created,
            "updatedAt": session.time.updated,
        }))
        .collect::<Vec<_>>();
    Json(json!({"sessions": sessions}))
}

pub async fn create_workspace_session(State(state): State<SharedState>) -> Json<Value> {
    let session = make_session(&state, Some("build".to_string()));
    state
        .sessions
        .write()
        .insert(session.id.clone(), session.clone());
    emit(&state, "session.created", json!({"info": session}));
    Json(json!({"session": session, "eventCursor": Value::Null}))
}

pub async fn session_status(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Response {
    if !state.sessions.read().contains_key(&session_id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(json!({
        "id": session_id,
        "state": if crate::state::lookup_run(&state, &session_id).is_some() { "busy" } else { "idle" },
        "modelID": "",
        "providerID": "",
        "agent": "build",
        "startedAt": chrono::Utc::now().timestamp_millis(),
        "updatedAt": chrono::Utc::now().timestamp_millis(),
    }))
    .into_response()
}

pub async fn accepted() -> (StatusCode, Json<Value>) {
    (StatusCode::ACCEPTED, Json(json!({"accepted": true})))
}

pub async fn not_implemented() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({"error": "not implemented"})),
    )
}
