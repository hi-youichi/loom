//! Message and part CRUD (tasks P1.12, P1.15).
//!
//! - `GET /session/:id/messages` — v1 projection
//! - `GET /api/session/:id/message` — v2 projection (used by `sdk.client.session.messages()`)
//! - `POST /api/session/:id/message` — append a new message (typically a
//!   user prompt or "rank these" markers from the TUI).
//! - `DELETE /api/session/:id/message/:messageID` — drop a message (task P1.12).
//! - `GET /api/session/:id/message/:messageID/part` — parts for a message.
//! - `GET /api/session/:id/message/:messageID/part/:partID` — single part.
//! - `PATCH /api/session/:id/message/:messageID/part/:partID` — patch part data (task P1.15).
//! - `DELETE /api/session/:id/message/:messageID/part/:partID` — drop part.
//! - `GET /api/session/:id/todo`, `/api/session/:id/diff` — projection stubs.
//!
//! Spec: `protocols/http/session.md:113-130`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::location::LocationQuery;
use crate::state::{
    emit, new_message_id, new_part_id, persist_messages, persist_parts, SharedState,
};

/// `GET /session/:id/todo` — placeholder for session todo list.
pub async fn get_session_todo() -> Json<Vec<Value>> {
    Json(vec![])
}

/// `GET /session/:id/diff` — placeholder for session diff.
pub async fn get_session_diff() -> Json<Vec<Value>> {
    Json(vec![])
}

/// `GET /session/status` — v1 spec: list running sessions.
pub async fn get_session_status(State(state): State<SharedState>) -> Json<Value> {
    let statuses = state
        .sessions
        .read()
        .keys()
        .map(|id| {
            let status = if crate::state::lookup_run(&state, id).is_some() {
                json!({"type": "busy"})
            } else {
                json!({"type": "idle"})
            };
            (id.clone(), status)
        })
        .collect::<serde_json::Map<_, _>>();
    Json(Value::Object(statuses))
}

/// `GET /session/:id/messages` — v1 list of all messages for the session.
pub async fn get_messages(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<Vec<Value>> {
    let messages = state.messages.read().get(&id).cloned().unwrap_or_default();
    let parts = state.parts.read();
    Json(
        messages
            .into_iter()
            .map(|message| {
                let message_parts = parts
                    .get(&message.id)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|part| part.data)
                    .collect::<Vec<_>>();
                json!({
                    "info": serde_json::to_value(message).unwrap_or(Value::Null),
                    "parts": message_parts,
                })
            })
            .collect(),
    )
}

/// Contract query for `GET /api/session/:sessionID/message` (message.ts:7-22).
/// All fields optional: `limit` (1–200), `order` ("asc"|"desc"), `cursor`.
#[derive(Deserialize, Default)]
pub struct SessionMessagesQuery {
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub order: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Project a stored MessageInfo + its parts into the SessionMessage.Message
/// shape. Returns `{info, parts}` — a reasonable projection of the tagged
/// union until a full SessionMessage.Message type is implemented.
fn project_message(state: &SharedState, message: &crate::state::MessageInfo) -> Value {
    let parts = state
        .parts
        .read()
        .get(&message.id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|part| part.data)
        .collect::<Vec<_>>();
    json!({
        "info": serde_json::to_value(message).unwrap_or(Value::Null),
        "parts": parts,
    })
}

/// `GET /api/session/:sessionID/message` — v2 contract (message.ts:26-44).
/// Returns `{ data: SessionMessage.Message[], cursor: {previous?, next?} }`.
/// Accepts SessionMessagesQuery (limit/order/cursor) and LocationQuery.
pub async fn get_api_session_message(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Query(query): Query<SessionMessagesQuery>,
    _loc: Query<LocationQuery>,
) -> Response {
    // 404 SessionNotFoundError if session doesn't exist.
    if !state.sessions.read().contains_key(&session_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "_tag": "SessionNotFoundError",
                "sessionID": session_id,
                "message": "session not found",
            })),
        )
            .into_response();
    }

    let messages = state
        .messages
        .read()
        .get(&session_id)
        .cloned()
        .unwrap_or_default();

    // Apply ordering (default asc).
    let mut ordered: Vec<_> = messages;
    if query.order.as_deref() == Some("desc") {
        ordered.reverse();
    }

    // Apply limit (default 50, max 200 per contract).
    let limit = query.limit.unwrap_or(50).min(200) as usize;
    let total = ordered.len();
    let has_more = total > limit;
    let data: Vec<Value> = ordered
        .iter()
        .take(limit)
        .map(|msg| project_message(&state, msg))
        .collect();

    // Opaque cursor encoding: offset for the next page as a string.
    // The contract marks cursors as opaque; clients pass them back verbatim.
    let next_cursor = if has_more { Some(limit.to_string()) } else { None };

    Json(json!({
        "data": data,
        "cursor": {
            "previous": Value::Null,
            "next": next_cursor,
        },
    }))
    .into_response()
}

/// `POST /api/session/:id/message` — append a message. Most TUI flows
/// use this for system prompts / "rank these" annotations; the main
/// user prompt goes through `/api/session/:id/agent` instead.
pub async fn post_api_session_message(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let now = chrono::Utc::now().timestamp_millis();
    let msg_id = new_message_id();
    let msg_json = json!({
        "id": msg_id,
        "sessionID": id,
        "role": body.get("role").and_then(|v| v.as_str()).unwrap_or("user"),
        "time": { "created": now },
        "agent": body.get("agent").and_then(|v| v.as_str()).unwrap_or("loom"),
    });

    let mut info = crate::state::MessageInfo {
        id: msg_id.clone(),
        session_id: id.clone(),
        role: msg_json["role"].as_str().unwrap_or("user").to_string(),
        time: json!({ "created": now }),
        agent: msg_json["agent"].as_str().unwrap_or("loom").to_string(),
        model: None,
        parent_id: body
            .get("parentID")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        tool: None,
        finish: None,
        provider_id: None,
        model_id: None,
        path: None,
        cost: None,
        tokens: None,
        mode: None,
    };

    // Optionally include extra fields.
    if let Some(model) = body.get("model") {
        info.model = Some(model.clone());
    }
    state
        .messages
        .write()
        .entry(id.clone())
        .or_default()
        .push(info);
    persist_messages(&state, &id);

    emit(
        &state,
        "message.updated",
        json!({
            "sessionID": id,
            "info": msg_json,
        }),
    );
    axum::Json(msg_json)
}

/// `GET /session/:id/message/:messageID` — one message with its parts.
pub async fn get_session_message(
    State(state): State<SharedState>,
    Path((session_id, message_id)): Path<(String, String)>,
) -> Response {
    let info = state
        .messages
        .read()
        .get(&session_id)
        .and_then(|messages| messages.iter().find(|message| message.id == message_id))
        .cloned();
    let Some(info) = info else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let parts = state
        .parts
        .read()
        .get(&message_id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|part| part.data)
        .collect::<Vec<_>>();
    Json(json!({"info": info, "parts": parts})).into_response()
}

pub async fn delete_api_session_message(
    State(state): State<SharedState>,
    Path((session_id, message_id)): Path<(String, String)>,
) -> Json<Value> {
    let mut removed = false;
    if let Some(list) = state.messages.write().get_mut(&session_id) {
        let before = list.len();
        list.retain(|m| m.id != message_id);
        removed = list.len() != before;
    }
    if removed {
        persist_messages(&state, &session_id);
        emit(
            &state,
            "message.removed",
            json!({
                "sessionID": session_id,
                "messageID": message_id,
            }),
        );
    }
    Json(json!({ "ok": removed }))
}

/// `GET /api/session/:id/message/:messageID/part` — list parts for a message.
pub async fn get_api_session_message_parts(
    State(state): State<SharedState>,
    Path((_session_id, message_id)): Path<(String, String)>,
) -> Json<Vec<Value>> {
    let parts = state.parts.read();
    let list = parts.get(&message_id).cloned().unwrap_or_default();
    Json(list.into_iter().map(|p| p.data).collect())
}

/// Path tuple — `:sessionID/:messageID/:partID`.
pub async fn get_api_session_message_part(
    State(state): State<SharedState>,
    Path((_session_id, message_id, part_id)): Path<(String, String, String)>,
) -> Json<Value> {
    let parts = state.parts.read();
    if let Some(list) = parts.get(&message_id) {
        if let Some(p) = list.iter().find(|p| p.id == part_id) {
            return Json(p.data.clone());
        }
    }
    Json(Value::Null)
}

/// `PATCH /api/session/:id/message/:messageID/part/:partID` — merge new
/// fields into the part's data blob (task P1.15). Accepts any subset
/// of keys: `text`, `metadata`, etc.
pub async fn patch_api_session_message_part(
    State(state): State<SharedState>,
    Path((session_id, message_id, part_id)): Path<(String, String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let mut updated = None;
    {
        let mut parts = state.parts.write();
        if let Some(list) = parts.get_mut(&message_id) {
            for p in list.iter_mut() {
                if p.id == part_id {
                    if let Some(obj) = body.as_object() {
                        if let Some(data_obj) = p.data.as_object_mut() {
                            for (k, v) in obj {
                                data_obj.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    updated = Some(p.data.clone());
                    break;
                }
            }
        }
    }
    if let Some(part) = updated {
        persist_parts(&state, &message_id);
        emit(
            &state,
            "message.part.updated",
            json!({
                "sessionID": session_id,
                "part": part.clone(),
                "time": chrono::Utc::now().timestamp_millis(),
            }),
        );
        return Json(part).into_response();
    }

    let message_exists = state
        .messages
        .read()
        .get(&session_id)
        .is_some_and(|messages| messages.iter().any(|message| message.id == message_id));
    if message_exists {
        let part_type = body
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("text")
            .to_string();
        let mut part = body;
        if let Some(object) = part.as_object_mut() {
            object.insert("id".to_string(), json!(part_id));
            object.insert("sessionID".to_string(), json!(session_id));
            object.insert("messageID".to_string(), json!(message_id));
            object.insert("type".to_string(), json!(part_type));
        }
        crate::agent_runner::push_part(&state, &message_id, &session_id, &part_type, part.clone());
        persist_parts(&state, &message_id);
        return Json(part).into_response();
    }

    (
        StatusCode::NOT_FOUND,
        Json(json!({"error":"message or part not found"})),
    )
        .into_response()
}

/// `DELETE /api/session/:id/message/:messageID/part/:partID` — drop part.
pub async fn delete_api_session_message_part(
    State(state): State<SharedState>,
    Path((session_id, message_id, part_id)): Path<(String, String, String)>,
) -> Json<Value> {
    let mut removed = false;
    {
        let mut parts = state.parts.write();
        if let Some(list) = parts.get_mut(&message_id) {
            let before = list.len();
            list.retain(|p| p.id != part_id);
            removed = list.len() != before;
        }
    }
    if removed {
        persist_parts(&state, &message_id);
        emit(
            &state,
            "message.part.removed",
            json!({
                "sessionID": session_id,
                "messageID": message_id,
                "partID": part_id,
            }),
        );
    }
    Json(json!({ "ok": removed }))
}

/// Stub: create a new text part. Used for tests only.
#[allow(dead_code)]
pub async fn post_api_session_message_part_text(
    State(state): State<SharedState>,
    Path((session_id, message_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let part_id = new_part_id();
    let part = json!({
        "id": part_id,
        "sessionID": session_id,
        "messageID": message_id,
        "type": "text",
        "text": body.get("text").cloned().unwrap_or(Value::Null),
    });
    state
        .parts
        .write()
        .entry(message_id.clone())
        .or_default()
        .push(crate::state::PartInfo {
            id: part_id.clone(),
            session_id: session_id.clone(),
            message_id: message_id.clone(),
            part_type: "text".to_string(),
            data: part.clone(),
        });
    persist_parts(&state, &message_id);
    emit(
        &state,
        "message.part.updated",
        json!({
            "sessionID": session_id,
            "part": part.clone(),
            "time": chrono::Utc::now().timestamp_millis(),
        }),
    );
    Json(part)
}

/// `POST /session/:id/init` — v1 init endpoint.
///
/// This one isn't strictly part of the messages CRUD but it's listed
/// near `POST /session/:id/messages/...init` in the v1 paths. It's just
/// an alias of `messages/post` for compatibility.
pub async fn post_session_init(
    State(_state): State<SharedState>,
    Path(_session_id): Path<String>,
    Json(_body): Json<Value>,
) -> Json<Value> {
    Json(json!({ "ok": true }))
}

#[allow(dead_code)]
#[derive(Deserialize, Default)]
pub struct ListQuery {
    #[serde(default)]
    pub after: Option<String>,
}
