//! TUI side-channel handlers.
//!
//! These exist so the opencode v1 TUI (which posts to `/tui/*` routes
//! instead of the canonical `/api/*` or `/session/*` endpoints) gets a
//! non-stub response. Each handler bridges the TUI-specific body shape
//! into the canonical session/event pipeline as best it can without the
//! TUI-level URL path parameters that the canonical handlers expect.
//!
//! Safety note: the canonical prompt agent-loop (handlers::session::prompt
//! → run_prompt) takes `Path<String>` for `sessionID`, so we cannot call
//! it directly from a body-driven TUI handler. We instead persist the
//! user message + emit the session.prompt event so downstream SSE
//! listeners pick up the new turn. The actual LLM call is not started
//! here — that responsibility stays with `/session/:id/prompt` and
//! `/api/session/:sessionID/prompt`, which are the canonical paths.

use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};

use crate::state::{emit, persist_messages, SharedState};

/// `POST /tui/submit-prompt` — TUI submits a user prompt.
///
/// Expected body: `{ sessionID: string, agent?: string, parts?: Part[] }`.
/// Where `Part` matches the v1 schema (`{ type: "text", text: string }`).
///
/// On success returns `200 { ok: true, messageID: string }`. The canonical
/// prompt agent-loop is NOT invoked from this handler — see the module
/// docstring for why. Downstream consumers listening on `/api/event` see
/// a `message.updated` event for the persisted user message.
pub async fn post_tui_submit_prompt(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let Some(session_id) = body.get("sessionID").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "name": "BadRequestError",
                "message": "sessionID is required",
            })),
        );
    };

    // Verify session exists (else 404 with the same discriminator TUI
    // sees from /session/:id/prompt).
    let exists = state.sessions.read().contains_key(session_id);
    if !exists {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "name": "SessionNotFoundError",
                "sessionID": session_id,
                "message": "session not found",
            })),
        );
    }

    let agent_name = body
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or("build")
        .to_string();
    let parts = body
        .get("parts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let user_message = json!({
        "sessionID": session_id,
        "role": "user",
        "agent": agent_name,
        "time": { "created": chrono::Utc::now().timestamp_millis() },
        "parts": parts,
    });
    let message_id = user_message
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("msg_{}", chrono::Utc::now().timestamp_millis()));

    // Persist the user message into the in-memory message map so subsequent
    // GET /session/:id/messages sees it. Then flush to disk via
    // persist_messages (best-effort: any IO error is logged at warn, not 500).
    {
        let mut msgs = state.messages.write();
        let entry = msgs.entry(session_id.to_string()).or_default();
        entry.push(crate::state::MessageInfo {
            id: message_id.clone(),
            session_id: session_id.to_string(),
            role: "user".to_string(),
            time: user_message
                .get("time")
                .cloned()
                .unwrap_or_else(|| json!({"created": chrono::Utc::now().timestamp_millis()})),
            agent: agent_name.clone(),
            model: None,
            parent_id: None,
            tool: None,
            finish: None,
            provider_id: None,
            model_id: None,
            path: None,
            cost: None,
            tokens: None,
            mode: None,
            ..Default::default()
        });
    }
    persist_messages(&state, session_id);

    emit(
        &state,
        "message.updated",
        json!({
            "sessionID": session_id,
            "info": user_message,
        }),
    );
    // The session.prompt event signals "a user prompt was admitted" so
    // any background agent runner listening on /api/event can pick it up
    // and start the LLM call (mirrors what prompt_v2 emits upstream).
    emit(
        &state,
        "session.prompt",
        json!({
            "sessionID": session_id,
            "messageID": message_id,
        }),
    );

    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "messageID": message_id,
            "note": "user message persisted; agent loop not started from this handler — use /api/session/:sessionID/prompt",
        })),
    )
}