//! Permission request handling (task LS-010 + W2 session-scoped conformance).
//!
//! Backed by a real in-memory store (`AppState::permissions`). When a tool
//! needs approval, a permission request is created and a `permission.asked` /
//! `permission.v2.asked` event is broadcast so the TUI can render a prompt.
//! The user replies via the reply routes, which transition the status and
//! emit `permission.replied` / `permission.v2.replied`. No approvals are
//! faked — an empty pending list is returned when no requests exist.
//!
//! ## W2 conformance (groups/permission.ts)
//!
//! Session-scoped endpoints implemented to match the opencode contract:
//! - `POST   /api/session/:sessionID/permission` — create (evaluate → effect)
//! - `GET    /api/session/:sessionID/permission` — list pending for session
//! - `GET    /api/session/:sessionID/permission/:requestID` — get one
//! - `POST   /api/session/:sessionID/permission/:requestID/reply` — reply (204)
//! - `GET    /api/permission/request` — global list (Location.response envelope)
//!
//! ## Enforcement boundary (honest status)
//!
//! The store and these endpoints are fully real, but **bridging a permission
//! decision into actual Loom tool execution is BLOCKED on LS-011**: the Loom
//! runner currently exposes no per-tool permission hook, so nothing in the
//! run path calls into this store yet. `create_session_permission` evaluates
//! against no configured ruleset (none exists), so the honest default effect
//! is `"ask"` — it always creates a real pending request and never silently
//! approves. We do not fake enforcement.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::location::{location_response, LocationQuery};
use crate::state::SharedState;

// ===========================================================================
// Contract-shaped projection helpers
// ===========================================================================

/// Project the internal `PermissionRequest` to the contract
/// `Permission.Request` shape `{ id, sessionID, action, resources, save?,
/// metadata?, source? }` (schema/permission.ts:34-37).
///
/// The internal struct stores `action` in the `tool` field and the contract
/// extras (`resources`/`save`/`metadata`/`source`) in the `input` JSON blob
/// because we cannot extend `state::PermissionRequest` (owned by state.rs).
fn to_contract_request(req: &crate::state::PermissionRequest) -> Value {
    let mut v = json!({
        "id": req.id,
        "sessionID": req.session_id,
        "action": req.tool,
        "resources": req.input.get("resources").cloned().unwrap_or(json!([])),
    });
    if let Some(save) = req.input.get("save") {
        v.as_object_mut().unwrap().insert("save".into(), save.clone());
    }
    if let Some(metadata) = req.input.get("metadata") {
        v.as_object_mut()
            .unwrap()
            .insert("metadata".into(), metadata.clone());
    }
    if let Some(source) = req.input.get("source") {
        v.as_object_mut()
            .unwrap()
            .insert("source".into(), source.clone());
    }
    v
}

/// `SessionNotFoundError` → HTTP 404 `{ _tag, sessionID, message }`
/// (errors.ts:55-62).
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

/// `PermissionNotFoundError` → HTTP 404 `{ _tag, requestID, message }`
/// (errors.ts:80-87).
fn permission_not_found(request_id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "_tag": "PermissionNotFoundError",
            "requestID": request_id,
            "message": format!("permission request {request_id} not found"),
        })),
    )
        .into_response()
}

fn session_exists(state: &SharedState, session_id: &str) -> bool {
    state.sessions.read().contains_key(session_id)
}

// ===========================================================================
// Global list endpoints (Location.response envelope)
// ===========================================================================

/// `GET /permission` — list pending permission requests (v1).
pub async fn get_permission_pending(State(state): State<SharedState>) -> Json<Vec<Value>> {
    let pending: Vec<Value> = state
        .permissions
        .read()
        .values()
        .filter(|req| req.status == "pending")
        .map(to_contract_request)
        .collect();
    Json(pending)
}

/// `GET /api/permission/request` (+ `/api/permission/pending`) — list pending
/// permission requests for a location (contract `permission.request.list`,
/// groups/permission.ts:23-34).
///
/// Wraps the result in `Location.response(Permission.Request[])` =
/// `{ location: Location.Info, data: Permission.Request[] }`.
pub async fn get_api_permission_pending(
    State(state): State<SharedState>,
    _loc: LocationQuery,
) -> Json<Value> {
    let pending: Vec<Value> = state
        .permissions
        .read()
        .values()
        .filter(|req| req.status == "pending")
        .map(to_contract_request)
        .collect();
    location_response(&state, pending)
}

// ===========================================================================
// Global create (v1 compat — NOT a contract endpoint)
// ===========================================================================

/// `POST /api/permission` — create a permission request (v1 compat shape).
///
/// Accepts `{ "sessionID", "tool", "input" }` and inserts a new pending
/// `PermissionRequest`, emitting `permission.asked` on the SSE bus.
pub async fn post_api_permission(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let now = chrono::Utc::now().timestamp_millis();
    let request = crate::state::PermissionRequest {
        id: crate::state::new_permission_id(),
        session_id: body
            .get("sessionID")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        tool: body
            .get("tool")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        input: body.get("input").cloned().unwrap_or(json!({})),
        status: "pending".to_string(),
        time_created: now,
    };

    let session_id = request.session_id.clone();
    let payload = to_contract_request(&request);
    state
        .permissions
        .write()
        .insert(request.id.clone(), request);

    crate::state::emit(
        &state,
        "permission.asked",
        json!({ "sessionID": session_id, "request": payload.clone() }),
    );

    (StatusCode::CREATED, Json(payload))
}

// ===========================================================================
// Global reply (v1 compat — NOT a contract endpoint)
// ===========================================================================

/// `POST /permission/:requestID/reply` — user answers a permission request (v1).
pub async fn post_permission_reply(
    State(state): State<SharedState>,
    Path(request_id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    transition_permission(&state, &request_id, &body)
}

/// `POST /api/permission/:requestID/reply` — v2 global alias.
pub async fn post_api_permission_reply(
    State(state): State<SharedState>,
    Path(request_id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    transition_permission(&state, &request_id, &body)
}

/// Shared reply logic for the v1/global reply endpoints: map the user's
/// `reply`/`status` to `approved`/`denied`, update the store, and emit
/// `permission.replied`. (The session-scoped contract reply uses a different
/// vocabulary — "once"/"always"/"reject" — and 204 NoContent.)
fn transition_permission(
    state: &SharedState,
    request_id: &str,
    body: &Value,
) -> (StatusCode, Json<Value>) {
    let raw = body
        .get("status")
        .and_then(Value::as_str)
        .or_else(|| body.get("reply").and_then(Value::as_str))
        .unwrap_or("deny");
    let new_status = match raw.to_ascii_lowercase().as_str() {
        "allow" | "approve" | "approved" | "once" | "always" => "approved",
        _ => "denied",
    };

    let mut permissions = state.permissions.write();
    let Some(request) = permissions.get_mut(request_id) else {
        drop(permissions);
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "permission request not found", "requestID": request_id })),
        );
    };
    request.status = new_status.to_string();
    let updated = request.clone();
    drop(permissions);

    crate::state::emit(
        state,
        "permission.replied",
        json!({
            "sessionID": updated.session_id,
            "requestID": request_id,
            "status": new_status,
        }),
    );

    (
        StatusCode::OK,
        Json(to_contract_request(&updated)),
    )
}

// ===========================================================================
// Session-scoped contract endpoints (groups/permission.ts:63-136)
// ===========================================================================

/// `POST /api/session/:sessionID/permission` — `session.permission.create`
/// (groups/permission.ts:63-87).
///
/// Accepts `{ id?, action, resources, save?, metadata?, source?, agent? }`.
/// Evaluates against the session's ruleset to produce a `Permission.Effect`.
///
/// **Honest behavior:** loom-server has no configured permission ruleset or
/// policy engine (LS-011 bridge is not built), so the default effect is
/// `"ask"` — it creates a real pending `Permission.Request` and never silently
/// approves. When a ruleset is added in the future, this logic can short-
/// circuit to `"allow"`/`"deny"` without creating a request.
///
/// Success: `200 { data: { id, effect } }`. Error: 404 `SessionNotFoundError`.
pub async fn create_session_permission(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if !session_exists(&state, &session_id) {
        return session_not_found(&session_id);
    }

    let id = body
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("per_{}", uuid::Uuid::new_v4().simple()));

    let now = chrono::Utc::now().timestamp_millis();
    let input = json!({
        "resources": body.get("resources").cloned().unwrap_or(json!([])),
        "save": body.get("save").cloned(),
        "metadata": body.get("metadata").cloned(),
        "source": body.get("source").cloned(),
    });

    let req = crate::state::PermissionRequest {
        id: id.clone(),
        session_id: session_id.clone(),
        tool: body
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        input,
        status: "pending".to_string(),
        time_created: now,
    };

    let contract_req = to_contract_request(&req);
    state.permissions.write().insert(id.clone(), req);

    crate::state::emit(
        &state,
        "permission.v2.asked",
        json!({ "sessionID": session_id, "request": contract_req }),
    );

    // No ruleset → honest default "ask" (always prompt, never auto-approve).
    (
        StatusCode::OK,
        Json(json!({ "data": { "id": id, "effect": "ask" } })),
    )
        .into_response()
}

/// `GET /api/session/:sessionID/permission` — `session.permission.list`
/// (groups/permission.ts:89-101).
///
/// Success: `200 { data: Permission.Request[] }` (plain envelope, NO location).
/// Error: 404 `SessionNotFoundError`.
pub async fn list_session_permissions(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Response {
    if !session_exists(&state, &session_id) {
        return session_not_found(&session_id);
    }
    let pending: Vec<Value> = state
        .permissions
        .read()
        .values()
        .filter(|req| req.session_id == session_id && req.status == "pending")
        .map(to_contract_request)
        .collect();
    (StatusCode::OK, Json(json!({ "data": pending }))).into_response()
}

/// `GET /api/session/:sessionID/permission/:requestID` — `session.permission.get`
/// (groups/permission.ts:103-116).
///
/// Success: `200 { data: Permission.Request }`.
/// Errors: 404 `SessionNotFoundError`, 404 `PermissionNotFoundError`.
pub async fn get_session_permission(
    State(state): State<SharedState>,
    Path((session_id, request_id)): Path<(String, String)>,
) -> Response {
    if !session_exists(&state, &session_id) {
        return session_not_found(&session_id);
    }
    let permissions = state.permissions.read();
    let Some(req) = permissions.get(&request_id) else {
        drop(permissions);
        return permission_not_found(&request_id);
    };
    if req.session_id != session_id {
        drop(permissions);
        return permission_not_found(&request_id);
    }
    let payload = to_contract_request(req);
    (StatusCode::OK, Json(json!({ "data": payload }))).into_response()
}

/// `POST /api/session/:sessionID/permission/:requestID/reply` —
/// `session.permission.reply` (groups/permission.ts:118-136).
///
/// Payload: `{ reply: "once"|"always"|"reject", message? }`.
/// Success: `204 NoContent`. Errors: 404 `SessionNotFoundError`,
/// 404 `PermissionNotFoundError`.
pub async fn reply_session_permission(
    State(state): State<SharedState>,
    Path((session_id, request_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    if !session_exists(&state, &session_id) {
        return session_not_found(&session_id);
    }

    let reply = body
        .get("reply")
        .and_then(Value::as_str)
        .unwrap_or("reject");
    let new_status = match reply {
        "once" | "always" => "approved",
        _ => "denied",
    };

    let mut permissions = state.permissions.write();
    let Some(req) = permissions.get_mut(&request_id) else {
        drop(permissions);
        return permission_not_found(&request_id);
    };
    if req.session_id != session_id {
        drop(permissions);
        return permission_not_found(&request_id);
    }
    req.status = new_status.to_string();
    drop(permissions);

    crate::state::emit(
        &state,
        "permission.v2.replied",
        json!({
            "sessionID": session_id,
            "requestID": request_id,
            "reply": reply,
        }),
    );

    StatusCode::NO_CONTENT.into_response()
}
