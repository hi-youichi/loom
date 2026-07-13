//! Session CRUD + run control (tasks P1.8-16).
//!
//! v1 routes (`/session/...`) and v2 routes (`/api/session/...`) both
//! live in this module. Bodies are the same — only the route strings
//! differ. The router in `routes.rs` registers both names so either TUI
//! build can talk to the same kernel.
//!
//! Spec: `protocols/http/session.md:113-130` (list of fields).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde_json::{json, Value};

use crate::agent_runner::{run_agent, RunCompletion};
use crate::state::{
    begin_run, emit, end_run, lookup_run, make_session, new_message_id, new_part_id,
    new_session_id, MessageInfo, PartInfo, SessionInfo, SharedState,
};

// ───────────────────────── v1 session CRUD ─────────────────────────

/// `GET /session` — list all sessions in memory.
pub async fn list_sessions(State(state): State<SharedState>) -> Json<Vec<Value>> {
    let sessions = state.sessions.read();
    Json(
        sessions
            .values()
            .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
            .collect(),
    )
}

/// `GET /session/:id` — fetch a single session by id.
pub async fn get_session(State(state): State<SharedState>, Path(id): Path<String>) -> Response {
    let sessions = state.sessions.read();
    match sessions.get(&id) {
        Some(s) => Json(serde_json::to_value(s).unwrap_or(Value::Null)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `POST /session` — create a new session. Returns the full SessionInfo.
pub async fn create_session(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let agent = body["agent"].as_str().map(str::to_string);
    let session = make_session(&state, agent);

    state
        .sessions
        .write()
        .insert(session.id.clone(), session.clone());

    emit(
        &state,
        "session.created",
        json!({
            "sessionID": session.id,
            "info": serde_json::to_value(&session).unwrap_or(Value::Null),
        }),
    );

    Json(serde_json::to_value(&session).unwrap_or(Value::Null))
}

/// `PATCH /session/:id` — update session fields (title, agent, model,
/// workspaceID). Partial-update; missing keys stay unchanged (task P1.14).
pub async fn patch_session(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let mut sessions = state.sessions.write();
    let Some(session) = sessions.get_mut(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    apply_session_patch(session, &body);
    session.time.updated = Utc::now().timestamp_millis();

    let cloned = session.clone();
    drop(sessions);

    emit(
        &state,
        "session.updated",
        json!({
            "sessionID": id,
            "info": serde_json::to_value(&cloned).unwrap_or(Value::Null),
        }),
    );

    Json(serde_json::to_value(&cloned).unwrap_or(Value::Null)).into_response()
}

/// `DELETE /session/:id` — remove session.
pub async fn delete_session(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> StatusCode {
    let existed = state.sessions.write().remove(&id).is_some();
    if existed {
        // Drop per-session state too so a re-create is clean.
        if let Some(messages) = state.messages.write().remove(&id) {
            let mut parts = state.parts.write();
            for message in messages {
                parts.remove(&message.id);
            }
        }
        if let Some(run) = lookup_run(&state, &id) {
            run.cancel();
            end_run(&state, &id, run.generation());
        }
        emit(&state, "session.deleted", json!({"sessionID": id}));
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

/// `GET /session/:id/children` — return descendants (sub-sessions).
pub async fn get_session_children(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<Vec<Value>> {
    let sessions = state.sessions.read();
    let list: Vec<Value> = sessions
        .values()
        .filter(|s| s.parent_id.as_deref() == Some(id.as_str()))
        .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
        .collect();
    Json(list)
}

// ───────────────────────── v1 main path ─────────────────────────

/// `POST /session/:id/prompt` — synchronous run (task P1.10).
pub async fn prompt(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    run_prompt(state, session_id, body, /*async_mode=*/ false).await
}

/// `POST /session/:id/prompt_async` — fire-and-forget variant. Used
/// when the client wants SSE-delivered results. Most v1 clients don't
/// need this since the synchronous variant already streams deltas, but
/// we expose it for parity with the v2 spec.
pub async fn prompt_async(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let state_for_bg = state.clone();
    let body_clone = body.clone();
    let sid = session_id.clone();
    tokio::spawn(async move {
        let _ = run_prompt(state_for_bg, sid, body_clone, /*async_mode=*/ true).await;
    });
    Json(json!({ "ok": true }))
}

// ───────────────────────── v2 main path ─────────────────────────

/// `POST /api/session/:id/agent` — the v2 spec's preferred entry point.
/// Functionally identical to the v1 prompt handler (task P1.8).
pub async fn api_session_prompt(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    run_prompt(state, session_id, body, false).await
}

/// `POST /api/session/:id/command` — v2 command dispatch (task P1.9).
pub async fn api_session_command(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    run_command(state, session_id, body).await
}

/// `POST /api/session/:id/shell` — v2 single-shot shell exec (task P1.10).
pub async fn api_session_shell(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    run_shell(state, session_id, body).await
}

/// `POST /session/:id/command` — v1 command dispatch.
pub async fn session_command(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    run_command(state, session_id, body).await
}

/// `POST /session/:id/shell` — v1 shell exec.
pub async fn session_shell(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    run_shell(state, session_id, body).await
}

async fn run_command(state: SharedState, session_id: String, body: Value) -> Response {
    let command = body
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if command.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "command is required"})),
        )
            .into_response();
    }
    let arguments = body
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut prompt = json!({
        "agent": body.get("agent").and_then(Value::as_str).unwrap_or("build"),
        "parts": [{
            "type": "text",
            "text": format!("/{command} {arguments}").trim().to_string(),
        }],
    });
    if let Some(model) = body.get("model").and_then(Value::as_str) {
        if let Some((provider_id, model_id)) = model.split_once('/') {
            prompt["model"] = json!({"providerID": provider_id, "modelID": model_id});
        }
    }
    run_prompt(state, session_id, prompt, false).await
}

async fn run_shell(state: SharedState, session_id: String, body: Value) -> Response {
    let Some(session) = state.sessions.read().get(&session_id).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let command = body
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if command.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "command is required"})),
        )
            .into_response();
    }

    emit(
        &state,
        "session.status",
        json!({"sessionID": session_id, "status": {"type": "busy"}}),
    );
    let output = if cfg!(windows) {
        tokio::process::Command::new("cmd.exe")
            .args(["/D", "/S", "/C", &command])
            .current_dir(&session.directory)
            .output()
            .await
    } else {
        tokio::process::Command::new("sh")
            .args(["-c", &command])
            .current_dir(&session.directory)
            .output()
            .await
    };

    let now = Utc::now().timestamp_millis();
    let message_id = new_message_id();
    let (finish, text, exit_code) = match output {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            (
                if output.status.success() {
                    "stop"
                } else {
                    "error"
                },
                text,
                output.status.code(),
            )
        }
        Err(error) => ("error", error.to_string(), None),
    };
    let info = MessageInfo {
        id: message_id.clone(),
        session_id: session_id.clone(),
        role: "assistant".to_string(),
        time: json!({"created": now, "completed": Utc::now().timestamp_millis()}),
        agent: body
            .get("agent")
            .and_then(Value::as_str)
            .unwrap_or("build")
            .to_string(),
        model: body.get("model").cloned(),
        parent_id: None,
        tool: Some(json!({"command": command})),
        finish: Some(finish.to_string()),
        provider_id: body
            .get("model")
            .and_then(|model| model.get("providerID"))
            .and_then(Value::as_str)
            .map(str::to_string),
        model_id: body
            .get("model")
            .and_then(|model| model.get("modelID"))
            .and_then(Value::as_str)
            .map(str::to_string),
        path: Some(json!({"cwd": session.directory, "root": session.directory})),
        cost: Some(0.0),
        tokens: Some(json!({
            "input": 0, "output": 0, "reasoning": 0,
            "cache": {"read": 0, "write": 0}
        })),
        mode: Some("shell".to_string()),
    };
    state
        .messages
        .write()
        .entry(session_id.clone())
        .or_default()
        .push(info.clone());
    let part = json!({
        "id": new_part_id(),
        "type": "text",
        "text": text,
        "metadata": {"command": command, "exitCode": exit_code},
    });
    crate::agent_runner::push_part(&state, &message_id, &session_id, "text", part);
    emit(
        &state,
        "message.updated",
        json!({"sessionID": session_id, "info": info}),
    );
    emit(
        &state,
        "session.status",
        json!({"sessionID": session_id, "status": {"type": "idle"}}),
    );
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

// ───────────────────────── abort & interrupt (task P1.11) ─────────────────────────

/// `POST /session/:id/abort` — v1 abort. Cancels the run token if
/// present; clears the run token afterwards so a subsequent prompt can
/// start a new one.
pub async fn session_abort(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Json<Value> {
    let cancelled = match lookup_run(&state, &session_id) {
        Some(token) => {
            token.cancel();
            true
        }
        None => false,
    };
    if let Some(ref token) = lookup_run(&state, &session_id) {
        end_run(&state, &session_id, token.generation());
    }
    emit(
        &state,
        "session.status",
        json!({
            "sessionID": session_id,
            "status": { "type": "idle" }
        }),
    );
    Json(json!({ "ok": true, "cancelled": cancelled }))
}

/// `POST /api/session/:id/interrupt` — v2 alias of `/session/:id/abort`.
pub async fn api_session_interrupt(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Json<Value> {
    session_abort(State(state), Path(session_id)).await
}

// ───────────────────────── session lifecycle (P1.13) ─────────────────────────

/// `POST /session/:id/share` — mark a session as shared.
pub async fn post_session_share(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Response {
    let url = format!("https://example.com/share/{session_id}");
    let session = {
        let mut sessions = state.sessions.write();
        let Some(session) = sessions.get_mut(&session_id) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        session.share = Some(crate::state::ShareInfo { url });
        session.time.updated = Utc::now().timestamp_millis();
        session.clone()
    };
    emit(
        &state,
        "session.updated",
        json!({"sessionID": session_id, "info": session}),
    );
    Json(json!(session)).into_response()
}

/// `POST /api/session/:id/fork` — duplicate session.
pub async fn post_api_session_fork(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(_body): Json<Value>,
) -> Response {
    let parent = state.sessions.read().get(&session_id).cloned();
    let Some(parent) = parent else {
        return (StatusCode::NOT_FOUND, Json(json!({}))).into_response();
    };
    let mut child = make_session(&state, parent.agent.clone());
    child.parent_id = Some(parent.id.clone());
    child.title = format!("{} (fork)", parent.title);
    let id = child.id.clone();
    state.sessions.write().insert(id.clone(), child.clone());

    let parent_messages = state
        .messages
        .read()
        .get(&session_id)
        .cloned()
        .unwrap_or_default();
    let mut message_ids = std::collections::HashMap::new();
    let mut forked_messages = parent_messages
        .iter()
        .cloned()
        .map(|mut message| {
            let old_id = message.id.clone();
            message.id = new_message_id();
            message.session_id = id.clone();
            message_ids.insert(old_id, message.id.clone());
            message
        })
        .collect::<Vec<_>>();
    for message in &mut forked_messages {
        if let Some(parent_id) = &message.parent_id {
            message.parent_id = message_ids.get(parent_id).cloned();
        }
    }
    state
        .messages
        .write()
        .insert(id.clone(), forked_messages.clone());

    let parent_parts = state.parts.read().clone();
    let mut child_parts = Vec::new();
    for parent_message in &parent_messages {
        let Some(new_message_id) = message_ids.get(&parent_message.id) else {
            continue;
        };
        let parts = parent_parts
            .get(&parent_message.id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|mut part| {
                part.id = new_part_id();
                part.session_id = id.clone();
                part.message_id = new_message_id.clone();
                if let Some(object) = part.data.as_object_mut() {
                    object.insert("id".to_string(), json!(part.id));
                    object.insert("sessionID".to_string(), json!(id));
                    object.insert("messageID".to_string(), json!(new_message_id));
                }
                part
            })
            .collect::<Vec<_>>();
        child_parts.push((new_message_id.clone(), parts));
    }
    let mut parts = state.parts.write();
    for (message_id, message_parts) in child_parts {
        parts.insert(message_id, message_parts);
    }
    drop(parts);

    emit(
        &state,
        "session.created",
        json!({
            "sessionID": id,
            "info": serde_json::to_value(&child).unwrap_or(Value::Null),
        }),
    );
    Json(serde_json::to_value(&child).unwrap_or(Value::Null)).into_response()
}

/// `POST /api/session/:id/summarize` — TUI calls this when the user
/// asks for a session summary. For MVP we just acknowledge.
pub async fn post_api_session_summarize(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Json<Value> {
    emit(
        &state,
        "session.summarize",
        json!({ "sessionID": session_id }),
    );
    Json(json!({ "ok": true, "summary": "" }))
}

/// `POST /api/session/:id/init` — TUI's "create session in worktree"
/// endpoint. We always succeed (no worktree plumbing in MVP).
pub async fn post_api_session_init(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(_body): Json<Value>,
) -> Json<Value> {
    emit(&state, "session.init", json!({ "sessionID": session_id }));
    Json(json!({ "ok": true }))
}

/// `POST /api/session/:id/revert` — placeholder; real implementation
/// lives in `handlers/revert.rs`.
pub async fn post_api_session_revert(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(_body): Json<Value>,
) -> Json<Value> {
    emit(&state, "session.revert", json!({ "sessionID": session_id }));
    Json(json!({ "ok": true }))
}

/// `GET /api/session/:id/event` — incremental replay (task P2.17).
pub async fn get_api_session_event(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Query(query): Query<EventQuery>,
) -> Json<Value> {
    let events = crate::state::snapshot_replay(&state, query.after.as_deref())
        .into_iter()
        .filter(|event| {
            event
                .payload
                .properties
                .get("sessionID")
                .and_then(Value::as_str)
                .is_none_or(|id| id == session_id)
        })
        .collect::<Vec<_>>();
    let cursor = events.last().map(|event| event.payload.event_id.clone());
    Json(json!({
        "data": events,
        "cursor": cursor,
        "hasMore": false,
    }))
}

#[derive(serde::Deserialize, Default)]
pub struct EventQuery {
    #[serde(default)]
    pub after: Option<String>,
}

// ───────────────────────── helpers ─────────────────────────

/// Apply allowed patch keys in-place. Used by both PATCH /session/:id
/// and session.update events emitted by v2's `Location.setWorkspace`.
fn apply_session_patch(session: &mut SessionInfo, body: &Value) {
    if let Some(title) = body.get("title").and_then(|v| v.as_str()) {
        session.title = title.to_string();
    }
    if let Some(agent) = body.get("agent").and_then(|v| v.as_str()) {
        session.agent = Some(agent.to_string());
    }
    if let Some(wid) = body.get("workspaceID").and_then(|v| v.as_str()) {
        session.workspace_id = Some(wid.to_string());
    }
    if let Some(parent) = body.get("parentID").and_then(|v| v.as_str()) {
        session.parent_id = Some(parent.to_string());
    }
}

/// Apply common prompt-handling logic shared by v1 + v2 + async modes.
///
/// `async_mode` flips behaviour between synchronous (return response
/// body) and async (just kick off the run; response is OK).
pub(crate) async fn run_prompt(
    state: SharedState,
    session_id: String,
    body: Value,
    _async_mode: bool,
) -> Response {
    if !state.sessions.read().contains_key(&session_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "session not found"})),
        )
            .into_response();
    }

    let input_parts = body
        .get("parts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let text = input_parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "prompt requires a non-empty text part"})),
        )
            .into_response();
    }

    let model_ref = body.get("model").and_then(Value::as_object);
    let provider_id = model_ref
        .and_then(|model| model.get("providerID"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let model_id = model_ref
        .and_then(|model| model.get("modelID"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let model = provider_id
        .as_deref()
        .zip(model_id.as_deref())
        .map(|(provider, model)| format!("{provider}/{model}"))
        .or_else(|| std::env::var("LOOM_MODEL").ok());
    let agent_name = body
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or("build")
        .to_string();

    let now = Utc::now().timestamp_millis();
    let user_message_id = new_message_id();
    let assistant_message_id = new_message_id();
    let user_info = MessageInfo {
        id: user_message_id.clone(),
        session_id: session_id.clone(),
        role: "user".to_string(),
        time: json!({"created": now}),
        agent: agent_name.clone(),
        model: body.get("model").cloned(),
        parent_id: None,
        tool: None,
        finish: None,
        provider_id: provider_id.clone(),
        model_id: model_id.clone(),
        path: None,
        cost: None,
        tokens: None,
        mode: None,
    };
    let assistant_info = MessageInfo {
        id: assistant_message_id.clone(),
        session_id: session_id.clone(),
        role: "assistant".to_string(),
        time: json!({"created": now}),
        agent: agent_name,
        model: body.get("model").cloned(),
        parent_id: Some(user_message_id.clone()),
        tool: None,
        finish: None,
        provider_id,
        model_id,
        path: Some(json!({
            "cwd": std::env::current_dir()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default(),
            "root": std::env::current_dir()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default(),
        })),
        cost: Some(0.0),
        tokens: Some(json!({
            "input": 0,
            "output": 0,
            "reasoning": 0,
            "cache": {"read": 0, "write": 0},
        })),
        mode: Some("build".to_string()),
    };
    state
        .messages
        .write()
        .entry(session_id.clone())
        .or_default()
        .extend([user_info.clone(), assistant_info.clone()]);

    let mut stored_user_parts = Vec::new();
    for input in input_parts {
        let part_id = input
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(new_part_id);
        let part_type = input
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("text")
            .to_string();
        let mut data = input;
        if let Some(object) = data.as_object_mut() {
            object.insert("id".to_string(), json!(part_id));
            object.insert("sessionID".to_string(), json!(session_id));
            object.insert("messageID".to_string(), json!(user_message_id));
            object.insert("type".to_string(), json!(part_type));
        }
        stored_user_parts.push(PartInfo {
            id: part_id,
            session_id: session_id.clone(),
            message_id: user_message_id.clone(),
            part_type,
            data,
        });
    }
    state
        .parts
        .write()
        .insert(user_message_id.clone(), stored_user_parts.clone());

    emit(
        &state,
        "message.updated",
        json!({"sessionID": session_id, "info": user_info}),
    );
    for part in &stored_user_parts {
        emit(
            &state,
            "message.part.updated",
            json!({"sessionID": session_id, "part": part.data}),
        );
    }
    emit(
        &state,
        "message.updated",
        json!({"sessionID": session_id, "info": assistant_info}),
    );
    emit(
        &state,
        "session.status",
        json!({"sessionID": session_id, "status": {"type": "busy"}}),
    );

    let cancellation = begin_run(&state, &session_id);
    let generation = cancellation.generation();
    let outcome = run_agent(
        state.clone(),
        session_id.clone(),
        assistant_message_id.clone(),
        text,
        model,
        cancellation,
    )
    .await;

    if let Ok(RunCompletion::Finished { reply }) = &outcome {
        let has_text = state
            .parts
            .read()
            .get(&assistant_message_id)
            .is_some_and(|parts| parts.iter().any(|part| part.part_type == "text"));
        if !has_text && !reply.is_empty() {
            crate::agent_runner::push_part(
                &state,
                &assistant_message_id,
                &session_id,
                "text",
                json!({"id": "text-0", "text": reply}),
            );
        }
    }

    let finish = match &outcome {
        Ok(RunCompletion::Finished { .. }) => "stop",
        Ok(RunCompletion::Cancelled) => "cancelled",
        Err(_) => "error",
    };
    let completed = Utc::now().timestamp_millis();
    let final_info = {
        let mut messages = state.messages.write();
        let message = messages.get_mut(&session_id).and_then(|messages| {
            messages
                .iter_mut()
                .find(|message| message.id == assistant_message_id)
        });
        if let Some(message) = message {
            message.finish = Some(finish.to_string());
            message.time = json!({"created": now, "completed": completed});
            serde_json::to_value(message).unwrap_or(Value::Null)
        } else {
            Value::Null
        }
    };
    emit(
        &state,
        "message.updated",
        json!({"sessionID": session_id, "info": final_info}),
    );
    emit(
        &state,
        "session.status",
        json!({"sessionID": session_id, "status": {"type": "idle"}}),
    );
    if let Err(error) = &outcome {
        emit(
            &state,
            "session.error",
            json!({
                "sessionID": session_id,
                "error": {"name": "UnknownError", "data": {"message": error}}
            }),
        );
    }
    end_run(&state, &session_id, generation);

    let parts = state
        .parts
        .read()
        .get(&assistant_message_id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|part| part.data)
        .collect::<Vec<_>>();
    let response = Json(json!({"info": final_info, "parts": parts}));
    match outcome {
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, response).into_response(),
        _ => response.into_response(),
    }
}

/// Helper shared by tests — create a session with given id.
///
/// Not used in production; only exercised by `tests/session_api.rs`.
#[allow(dead_code)]
pub async fn create_session_with_id(state: &SharedState, id: String) -> SessionInfo {
    let session = make_session(state, Some("loom".to_string()));
    let mut with_id = session;
    with_id.id = id.clone();
    with_id.slug = id.clone();
    state.sessions.write().insert(id.clone(), with_id.clone());
    with_id
}

/// Helper shared by tests — count sessions.
#[allow(dead_code)]
pub fn new_sid() -> String {
    new_session_id()
}
