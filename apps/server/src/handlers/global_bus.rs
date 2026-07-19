//! Global scope endpoints (task P2.23).

use std::path::PathBuf;

use axum::{extract::Path, http::StatusCode, Json};
use serde_json::{json, Value};

use crate::state::{emit, SharedState};

/// `GET /global/event/replay` — replay the global event buffer.
pub async fn get_global_event_replay(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> Json<Vec<Value>> {
    let snap = crate::state::snapshot_replay(&state, None);
    Json(
        snap.into_iter()
            .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
            .collect(),
    )
}

/// `GET /global/dispose` — attempt shutdown. Real shutdown happens
/// through SIGINT; this endpoint is just an acknowledgement so a TUI
/// final step doesn't hang.
pub async fn post_global_dispose(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> Json<Value> {
    let runs = state
        .abort_tokens
        .write()
        .drain()
        .map(|(_, run)| run)
        .collect::<Vec<_>>();
    for run in runs {
        run.cancel();
    }
    state.sessions.write().clear();
    state.messages.write().clear();
    state.parts.write().clear();
    state.event_buffer.write().clear();
    emit(&state, "server.instance.disposed", json!({}));
    Json(json!({ "ok": true, "shutdown": true }))
}

/// `GET /global/version` — version info.
pub async fn get_global_version() -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "kind": "external-kernel",
    }))
}

/// `POST /global/instance/update` — apply an instance update.
///
/// **Not supported** (task LS-017): this server runs as an external kernel
/// launched by the host process and has no self-update capability. Returns
/// 501 honestly rather than a success-shaped stub that would claim an update
/// was applied.
pub async fn post_global_instance_update(
    Json(_body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "error": "instance update is not supported" })),
    )
}

/// `POST /global/upgrade` — upgrade the running instance.
///
/// **Not supported** (task LS-017): same rationale as
/// `post_global_instance_update` — there is no in-process upgrade path for
/// an externally-launched kernel. Returns 501 honestly.
pub async fn post_global_upgrade() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "error": "upgrade is not supported" })),
    )
}

/// `GET /global/config` — alias of `/config`.
pub async fn get_global_config(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> Json<Value> {
    let cfg = state.config.read().clone();
    Json(serde_json::to_value(&cfg).unwrap_or(Value::Null))
}

/// `PATCH /global/config` — alias of PATCH /api/config.
pub async fn patch_global_config(
    axum::extract::State(state): axum::extract::State<SharedState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let response =
        super::bootstrap::patch_api_config(axum::extract::State(state.clone()), Json(body)).await;
    if state.persist_config {
        let config_text = {
            let config = state.config.read();
            toml::to_string_pretty(&*config)
        };
        match config_text {
            Ok(config_text) => {
                let path = loom_config_path();
                let result = async {
                    if let Some(parent) = path.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    tokio::fs::write(&path, config_text).await
                }
                .await;
                if let Err(error) = result {
                    tracing::warn!(%error, path = %path.display(), "failed to persist global config");
                }
            }
            Err(error) => tracing::warn!(%error, "failed to serialize global config"),
        }
        emit(
            &state,
            "server.config.changed",
            json!({}),
        );
    }
    response
}

fn loom_config_path() -> PathBuf {
    config::home::loom_home().join("config.toml")
}

/// `POST /session/:id/init` — project init handler (task P1.13).
pub async fn post_session_project_init(
    axum::extract::State(state): axum::extract::State<SharedState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Json(_body): Json<Value>,
) -> Json<Value> {
    emit(&state, "session.init", json!({"sessionID": session_id}));
    Json(json!({ "ok": true }))
}

/// `DELETE /global/session/:id` — alias of `DELETE /session/:id`.
pub async fn delete_global_session(
    axum::extract::State(state): axum::extract::State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<Value> {
    let existed = state.sessions.write().remove(&id).is_some();
    if existed {
        state.messages.write().remove(&id);
        emit(&state, "session.deleted", json!({"sessionID": id}));
    }
    Json(json!({ "ok": existed }))
}

/// `GET /global/session` — list sessions globally.
pub async fn get_global_session(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> Json<Vec<Value>> {
    let sessions = state.sessions.read();
    Json(
        sessions
            .values()
            .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
            .collect(),
    )
}

/// `GET /api/global/event` — alias of v2 channel for spec parity.
pub async fn get_api_global_event(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> Json<Vec<Value>> {
    let snap = crate::state::snapshot_replay(&state, None);
    Json(
        snap.into_iter()
            .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
            .collect(),
    )
}

/// `PATCH /api/global/event/:id` — sync an ack.
pub async fn patch_api_global_event_ack(Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "ok": true, "id": id }))
}
