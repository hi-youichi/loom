//! Revert APIs (task P2.19).
//!
//! The TUI uses these to undo a finished assistant turn or restore an
//! earlier session snapshot. We acknowledge with the current state —
//! check-pointing isn't wired up in MVP.

use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::{json, Value};

use crate::state::{emit, SharedState};

/// `POST /session/:id/revert` — revert the most recent assistant turn.
pub async fn post_session_revert(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(_body): Json<Value>,
) -> Json<Value> {
    emit(&state, "session.reverted", json!({"sessionID": session_id}));
    Json(json!({ "ok": true }))
}

/// `GET /session/:id/revert/stage` — peek at pending reverts.
pub async fn get_session_revert_stage(
    State(_state): State<SharedState>,
    Path(_session_id): Path<String>,
) -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `GET /api/session/:id/revert/stage` — v2 alias.
pub async fn get_api_session_revert_stage(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Json<Value> {
    get_session_revert_stage(State(state), Path(session_id)).await
}

/// `POST /api/session/:id/revert/stage` — stage a revert.
pub async fn post_api_session_revert_stage(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    emit(
        &state,
        "session.revert.stage",
        json!({
            "sessionID": session_id,
            "body": body,
        }),
    );
    Json(json!({ "ok": true }))
}

/// `POST /session/:id/revert/clear` — clear pending reverts.
pub async fn post_session_revert_clear(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Json<Value> {
    emit(
        &state,
        "session.revert.cleared",
        json!({"sessionID": session_id}),
    );
    Json(json!({ "ok": true }))
}

/// `POST /api/session/:id/revert/clear` — v2 alias.
pub async fn post_api_session_revert_clear(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Json<Value> {
    post_session_revert_clear(State(state), Path(session_id)).await
}

/// `POST /session/:id/revert/commit` — commit a staged revert.
pub async fn post_session_revert_commit(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(_body): Json<Value>,
) -> Json<Value> {
    emit(
        &state,
        "session.revert.committed",
        json!({"sessionID": session_id}),
    );
    Json(json!({ "ok": true }))
}

/// `POST /api/session/:id/revert/commit` — v2 alias.
pub async fn post_api_session_revert_commit(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    post_session_revert_commit(State(state), Path(session_id), Json(body)).await
}
