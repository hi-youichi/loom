//! VCS helpers (task P0.2).
//!
//! TUI may call these to discover branches / diffs. We return empty
//! results because loom-server in MVP is a single-workdir executor —
//! the Loom agent itself owns VCS operations via tools, and it doesn't
//! expose them over this HTTP surface yet. A follow-up can pipe `git`
//! command output through these endpoints.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::state::{emit, SharedState};

/// `GET /vcs` — list VCS providers.
pub async fn get_vcs() -> Json<Value> {
    Json(json!({
        "branch": "main",
        "providers": ["git"],
    }))
}

/// `GET /vcs/status` — repository status summary. Not implemented.
pub async fn get_vcs_status() -> Json<Value> {
    Json(json!({
        "dirty": false,
        "branch": "main",
        "ahead": 0,
        "behind": 0,
        "modified": [],
        "staged": [],
        "untracked": [],
    }))
}

/// `GET /vcs/diff` — textual diff. Not implemented.
pub async fn get_vcs_diff() -> Json<Value> {
    Json(json!({
        "diff": "",
    }))
}

/// `GET /vcs/diff/raw` — raw diff content. Not implemented.
pub async fn get_vcs_diff_raw() -> Json<Value> {
    Json(json!({
        "diff": "",
    }))
}

/// `POST /api/location/snapshot` — v2 spec lets the TUI tell the
/// kernel "I'm about to ask a question about this state". We log it
/// for parity and return the current project location.
pub async fn post_api_location_snapshot(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    tracing::info!(body = %body, "location snapshot");
    emit(&state, "location.snapshot", body.clone());
    Json(body)
}

#[allow(dead_code)]
pub async fn not_implemented(Path(_id): Path<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "message": "not implemented" })),
    )
}
