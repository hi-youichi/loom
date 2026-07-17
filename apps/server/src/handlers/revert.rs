//! Session revert endpoints (W2 conformance: groups/session.ts:256-290).
//!
//! Implements the three opencode `session.revert.*` contract endpoints:
//! - `POST /api/session/:sessionID/revert/stage`  — stage/move a revert
//!   boundary, optionally apply file changes → `{ data: Revert.State }`
//! - `POST /api/session/:sessionID/revert/clear`  — clear a staged revert → 204
//! - `POST /api/session/:sessionID/revert/commit` — commit a staged revert → 204
//!
//! ## Honest status: 501 (checkpoint store not connected)
//!
//! opencode implements revert on top of a durable **checkpoint store** — the
//! ReAct runner persists per-message snapshots that `stage` rewinds to,
//! `commit` finalizes, and `clear` discards. Loom's agent runner *does*
//! build a `Checkpointer` (see `agent/agent-core/.../checkpointer.rs`,
//! backed by `checkpoint_sqlite_store::SqliteSaver`), but that store is
//! internal to the ReAct runner and is **not surfaced to the server**.
//! `AppState` and `agent_runner` expose no API to list checkpoints, compute
//! file diffs, or restore a prior state. Consequently the revert endpoints
//! cannot perform real work yet.
//!
//! Rather than ship a success-shaped stub, every endpoint returns **501
//! Not Implemented** with a clear `UnknownError`-shaped reason once the
//! session is validated. When the checkpoint store is bridged into
//! `AppState` (a future task), these handlers flip to real behavior
//! without changing their signatures.
//!
//! Error precedence follows the contract: a missing session yields
//! `SessionNotFoundError` (404) first; a missing message (stage only)
//! yields `MessageNotFoundError` (404); only then is the 501 returned.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::location::LocationQuery;
use crate::state::SharedState;

/// `SessionNotFoundError` → HTTP 404 `{ _tag, sessionID, message }`
/// (errors.ts:55-62). Matches the `error: SessionNotFoundError` clause on
/// all three revert endpoints.
fn session_not_found(session_id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "_tag": "SessionNotFoundError",
            "sessionID": session_id,
            "message": format!("session {session_id} not found"),
        })),
    )
        .into_response()
}

/// `MessageNotFoundError` → HTTP 404 `{ _tag, sessionID, messageID, message }`
/// (errors.ts:64-72). Used by `session.revert.stage` when `messageID` is not
/// owned by the session.
fn message_not_found(session_id: &str, message_id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "_tag": "MessageNotFoundError",
            "sessionID": session_id,
            "messageID": message_id,
            "message": format!("message {message_id} not found in session {session_id}"),
        })),
    )
        .into_response()
}

/// Honest 501: the checkpoint store is not connected to the server, so the
/// revert operation cannot be performed. Shaped as `UnknownError` (503 in the
/// opencode error map is for *service* outages; revert-stage lists
/// `UnknownError` which maps to 500, but a not-yet-implemented feature is
/// semantically 501 Not Implemented per HTTP). The `_tag` is preserved for
/// client-side discriminator matching while the status reflects the real
/// reason.
fn not_implemented(session_id: &str, op: &str) -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "_tag": "UnknownError",
            "message": format!(
                "session.revert.{op} is not implemented: the Loom checkpoint store is internal to the agent runner and not connected to the server. Session {session_id} cannot be reverted yet."
            ),
        })),
    )
        .into_response()
}

fn session_exists(state: &SharedState, session_id: &str) -> bool {
    state.sessions.read().contains_key(session_id)
}

/// Check that a message belongs to a session (for `revert.stage` validation).
fn message_in_session(state: &SharedState, session_id: &str, message_id: &str) -> bool {
    state
        .messages
        .read()
        .get(session_id)
        .is_some_and(|msgs| msgs.iter().any(|m| m.id == message_id))
}

// ===========================================================================
// POST /api/session/:sessionID/revert/stage
// (groups/session.ts:256-270, session.revert.stage)
// ===========================================================================

/// Stage or move a reversible session boundary and optionally apply its file
/// changes.
///
/// Payload: `{ messageID: SessionMessage.ID, files?: boolean }`.
/// Success: `200 { data: Revert.State }` where `Revert.State` =
/// `{ messageID, partID?, snapshot?, diff?, files?: FileDiff[] }`
/// (schema/revert.ts:17-23).
/// Errors: `MessageNotFoundError` (404), `SessionNotFoundError` (404),
/// `UnknownError` (501 here — checkpoint store not connected).
pub async fn revert_stage(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    _loc: LocationQuery,
    Json(body): Json<Value>,
) -> Response {
    if !session_exists(&state, &session_id) {
        return session_not_found(&session_id);
    }

    let message_id = body
        .get("messageID")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !message_in_session(&state, &session_id, message_id) {
        return message_not_found(&session_id, message_id);
    }

    // Checkpoint store not connected → honest 501. A real implementation would
    // rewind to the checkpoint for `messageID`, compute the file diff, and
    // return `{ data: Revert.State }`.
    not_implemented(&session_id, "stage")
}

// ===========================================================================
// POST /api/session/:sessionID/revert/clear
// (groups/session.ts:271-279, session.revert.clear)
// ===========================================================================

/// Clear a staged revert.
///
/// Success: `204 NoContent`.
/// Errors: `SessionNotFoundError` (404), `UnknownError` (501 here).
pub async fn revert_clear(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    _loc: LocationQuery,
) -> Response {
    if !session_exists(&state, &session_id) {
        return session_not_found(&session_id);
    }

    // Checkpoint store not connected → honest 501.
    not_implemented(&session_id, "clear")
}

// ===========================================================================
// POST /api/session/:sessionID/revert/commit
// (groups/session.ts:280-290, session.revert.commit)
// ===========================================================================

/// Commit a staged revert.
///
/// Success: `204 NoContent`.
/// Errors: `SessionNotFoundError` (404).
pub async fn revert_commit(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    _loc: LocationQuery,
) -> Response {
    if !session_exists(&state, &session_id) {
        return session_not_found(&session_id);
    }

    // Checkpoint store not connected → honest 501. (The contract lists only
    // SessionNotFoundError for commit, but reverting without a checkpoint
    // store is impossible, so 501 is the honest response.)
    not_implemented(&session_id, "commit")
}
