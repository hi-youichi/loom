//! Question request handling (task P2.18 + W2 session-scoped conformance).
//!
//! Backed by a module-level in-memory store (there is no question store in
//! `AppState` — state.rs is outside this wave's scope, so a process-wide
//! `OnceLock<RwLock<HashMap>>` provides real persistence for the lifecycle).
//!
//! ## W2 conformance (groups/question.ts)
//!
//! Session-scoped endpoints implemented to match the opencode contract:
//! - `GET  /api/session/:sessionID/question` — list pending for session
//! - `POST /api/session/:sessionID/question/:requestID/reply` — reply (204)
//! - `POST /api/session/:sessionID/question/:requestID/reject` — reject (204)
//! - `GET  /api/question/request` — global list (Location.response envelope)
//!
//! All return real data from the store; no success-shaped stubs.

use std::collections::HashMap;
use std::sync::OnceLock;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::location::{location_response, LocationQuery};
use crate::state::SharedState;

// ===========================================================================
// Module-level question store
// ===========================================================================

/// `Question.Request` (schema/question.ts:52-57) = `{ id, sessionID, questions,
/// tool? }`. We add internal `status` / `time_created` fields (skipped during
/// serialization) so the lifecycle can track `"pending"` → `"answered"` /
/// `"rejected"` without leaking non-contract fields to the wire.
#[derive(Clone, Serialize, Deserialize)]
struct QuestionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// `Question.Info[]` — stored as a JSON blob.
    pub questions: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<serde_json::Value>,
    /// Internal lifecycle state — never serialized.
    #[serde(skip)]
    pub status: String,
    /// Internal — epoch millis (stored for future ordering; not read yet).
    #[serde(skip)]
    #[allow(dead_code)]
    pub time_created: i64,
}

/// Process-wide question store (no `AppState` field exists for questions;
/// `OnceLock` avoids adding an external crate dependency).
fn questions_store() -> &'static parking_lot::RwLock<HashMap<String, QuestionRequest>> {
    static STORE: OnceLock<parking_lot::RwLock<HashMap<String, QuestionRequest>>> = OnceLock::new();
    STORE.get_or_init(|| parking_lot::RwLock::new(HashMap::new()))
}

fn new_question_id() -> String {
    format!("que_{}", uuid::Uuid::new_v4().simple())
}

// ===========================================================================
// Error helpers (errors.ts)
// ===========================================================================

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

fn question_not_found(request_id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "_tag": "QuestionNotFoundError",
            "requestID": request_id,
            "message": format!("question request {request_id} not found"),
        })),
    )
        .into_response()
}

fn session_exists(state: &SharedState, session_id: &str) -> bool {
    state.sessions.read().contains_key(session_id)
}

// ===========================================================================
// Session-scoped contract endpoints (groups/question.ts:37-80)
// ===========================================================================

/// `GET /api/session/:sessionID/question` — `session.question.list`
/// (groups/question.ts:37-49).
///
/// Success: `200 { data: Question.Request[] }` (plain envelope, NO location).
/// Error: 404 `SessionNotFoundError`.
pub async fn list_session_questions(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Response {
    if !session_exists(&state, &session_id) {
        return session_not_found(&session_id);
    }
    let pending: Vec<Value> = questions_store()
        .read()
        .values()
        .filter(|q| q.session_id == session_id && q.status == "pending")
        .map(|q| serde_json::to_value(q).unwrap_or(Value::Null))
        .collect();
    (StatusCode::OK, Json(json!({ "data": pending }))).into_response()
}

/// `POST /api/session/:sessionID/question/:requestID/reply` —
/// `session.question.reply` (groups/question.ts:51-65).
///
/// Payload: `Question.Reply = { answers: string[][] }`.
/// Success: `204 NoContent`. Errors: 404 `SessionNotFoundError`,
/// 404 `QuestionNotFoundError`.
pub async fn reply_session_question(
    State(state): State<SharedState>,
    Path((session_id, request_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    if !session_exists(&state, &session_id) {
        return session_not_found(&session_id);
    }

    let answers = body.get("answers").cloned().unwrap_or(json!([]));

    let mut store = questions_store().write();
    let Some(q) = store.get_mut(&request_id) else {
        drop(store);
        return question_not_found(&request_id);
    };
    if q.session_id != session_id {
        drop(store);
        return question_not_found(&request_id);
    }
    q.status = "answered".to_string();
    drop(store);

    crate::state::emit(
        &state,
        "question.v2.replied",
        json!({
            "sessionID": session_id,
            "requestID": request_id,
            "answers": answers,
        }),
    );

    StatusCode::NO_CONTENT.into_response()
}

/// `POST /api/session/:sessionID/question/:requestID/reject` —
/// `session.question.reject` (groups/question.ts:67-80).
///
/// Success: `204 NoContent`. Errors: 404 `SessionNotFoundError`,
/// 404 `QuestionNotFoundError`.
pub async fn reject_session_question(
    State(state): State<SharedState>,
    Path((session_id, request_id)): Path<(String, String)>,
) -> Response {
    if !session_exists(&state, &session_id) {
        return session_not_found(&session_id);
    }

    let mut store = questions_store().write();
    let Some(q) = store.get_mut(&request_id) else {
        drop(store);
        return question_not_found(&request_id);
    };
    if q.session_id != session_id {
        drop(store);
        return question_not_found(&request_id);
    }
    q.status = "rejected".to_string();
    drop(store);

    crate::state::emit(
        &state,
        "question.v2.rejected",
        json!({
            "sessionID": session_id,
            "requestID": request_id,
        }),
    );

    StatusCode::NO_CONTENT.into_response()
}

// ===========================================================================
// Global list endpoint (Location.response envelope)
// ===========================================================================

/// `GET /api/question/request` (+ `/api/question/pending`) —
/// `question.request.list` (groups/question.ts:20-31).
///
/// Wraps the result in `Location.response(Question.Request[])` =
/// `{ location: Location.Info, data: Question.Request[] }`.
pub async fn get_api_question_pending(
    State(state): State<SharedState>,
    _loc: LocationQuery,
) -> Json<Value> {
    let pending: Vec<Value> = questions_store()
        .read()
        .values()
        .filter(|q| q.status == "pending")
        .map(|q| serde_json::to_value(q).unwrap_or(Value::Null))
        .collect();
    location_response(&state, pending)
}

// ===========================================================================
// v1 compat endpoints (NOT contract endpoints — real behavior, no stubs)
// ===========================================================================

/// `POST /api/question` — create a question request (v1 compat).
///
/// Accepts `{ sessionID, questions, tool? }` and inserts a real pending
/// `QuestionRequest`, emitting `question.asked` on the SSE bus.
pub async fn post_api_question(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let now = chrono::Utc::now().timestamp_millis();
    let req = QuestionRequest {
        id: new_question_id(),
        session_id: body
            .get("sessionID")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        questions: body.get("questions").cloned().unwrap_or(json!([])),
        tool: body.get("tool").cloned(),
        status: "pending".to_string(),
        time_created: now,
    };

    let payload = serde_json::to_value(&req).unwrap_or(json!({}));
    questions_store().write().insert(req.id.clone(), req);

    crate::state::emit(
        &state,
        "question.asked",
        json!({ "sessionID": payload.get("sessionID").cloned().unwrap_or(json!("")), "request": payload }),
    );

    (StatusCode::CREATED, Json(payload))
}

/// `POST /question` — v1 raising entry. Delegates to `post_api_question`.
pub async fn post_question(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    post_api_question(State(state), Json(body)).await
}

/// `GET /question/pending` — v1 listing (returns bare array).
pub async fn get_question_pending() -> Json<Value> {
    let pending: Vec<Value> = questions_store()
        .read()
        .values()
        .filter(|q| q.status == "pending")
        .map(|q| serde_json::to_value(q).unwrap_or(Value::Null))
        .collect();
    Json(json!(pending))
}

/// `POST /question/:requestID/reply` — user replies to a question (v1).
pub async fn post_question_reply(
    Path(request_id): Path<String>,
    Json(_body): Json<Value>,
) -> Json<Value> {
    let mut store = questions_store().write();
    if let Some(q) = store.get_mut(&request_id) {
        q.status = "answered".to_string();
    }
    drop(store);
    Json(json!(true))
}

/// `POST /api/question/:requestID/reply` — v2 global alias.
pub async fn post_api_question_reply(
    Path(request_id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    post_question_reply(Path(request_id), Json(body)).await
}

/// `POST /question/:requestID/reject` — reject a question (v1).
pub async fn post_question_reject(Path(request_id): Path<String>) -> Json<Value> {
    let mut store = questions_store().write();
    if let Some(q) = store.get_mut(&request_id) {
        q.status = "rejected".to_string();
    }
    drop(store);
    Json(json!(true))
}
