//! PTY HTTP handlers — opencode `server.pty` group
//! (`.loom/contract/group-pty.ts` + `schema-pty.ts`).
//!
//! Implements the full `/api/pty*` route surface:
//!
//! | Method | Path                              | Handler           | Success       |
//! |--------|-----------------------------------|-------------------|---------------|
//! | GET    | `/api/pty`                        | [`list`]          | `{ data: [Info] }` |
//! | POST   | `/api/pty`                        | [`create`]        | `Info` (201)  |
//! | GET    | `/api/pty/:ptyID`                 | [`get`]           | `Info`        |
//! | PUT    | `/api/pty/:ptyID`                 | [`update`]        | `Info`        |
//! | DELETE | `/api/pty/:ptyID`                 | [`remove`]        | 204           |
//! | POST   | `/api/pty/:ptyID/connect-token`   | [`connect_token`] | `{ ticket }`  |
//! | GET    | `/api/pty/:ptyID/connect`         | [`connect`]       | WS upgrade    |
//!
//! Process lifecycle is delegated to [`PtyManager`](crate::pty::PtyManager)
//! (task B, `apps/server/src/pty.rs`); connect tickets live in
//! [`AppState::pty_tickets`](crate::state::AppState) as single-use entries.
//! The WebSocket connect flow uses the `loom-pty-protocol` crate for replay
//! framing (`chunk_replay` + `meta_frame`) and input decoding (`decode_input`).
//!
//! ## Shared manager
//!
//! [`AppState`] has no `PtyManager` field (owned by a separate task), so a
//! module-level [`OnceLock`] holds one process-wide instance shared by all
//! handlers. This is sufficient for a single-server deployment.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::location::{location_response, LocationQuery};
use crate::pty::{CreateInput, PtyManager, PtySizeInput};
use crate::state::{new_pty_ticket, PtyTicket, SharedState};

/// PTY connect ticket lifetime in seconds (PtyTicket.ConnectToken.expires_in).
const TICKET_EXPIRES_IN: u64 = 60;

/// PtyNotFoundError-shaped 404 response body (errors.ts:104-111).
fn pty_not_found(id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "_tag": "PtyNotFoundError",
            "ptyID": id,
            "message": format!("pty session not found: {id}"),
        })),
    )
        .into_response()
}

// ───────────────────────── module state ──────────────────────────

/// Process-wide [`PtyManager`] (task B). All handlers share this instance.
fn manager() -> &'static PtyManager {
    static MGR: OnceLock<PtyManager> = OnceLock::new();
    MGR.get_or_init(PtyManager::new)
}

// ─────────────────────── constants (group-pty.ts) ─────────────────
//
// PTY_CONNECT_TICKET_QUERY  = "ticket"
// PTY_CONNECT_TOKEN_HEADER  = "x-opencode-ticket"
// PTY_CONNECT_TOKEN_HEADER_VALUE = "1"

#[allow(dead_code)] // documented constant; the field name drives serde parsing
const TICKET_QUERY: &str = "ticket";
const TOKEN_HEADER: &str = "x-opencode-ticket";
const TOKEN_HEADER_VALUE: &str = "1";

// ──────────────────────────── handlers ───────────────────────────

/// `Pty.CreateInput` body for `POST /api/pty` (schema-pty.ts).
#[derive(Debug, Default, Deserialize)]
pub struct CreateBody {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub size: Option<SizeBody>,
}

/// `Pty.UpdateInput` body for `PUT /api/pty/:ptyID` (schema-pty.ts).
#[derive(Debug, Default, Deserialize)]
pub struct UpdateBody {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub size: Option<SizeBody>,
}

/// Viewport `{ rows: PositiveInt, cols: PositiveInt }` (schema-pty.ts).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct SizeBody {
    pub rows: u16,
    pub cols: u16,
}

/// Query params for the connect route: `?ticket=tkt_…`.
#[derive(Debug, Default, Deserialize)]
pub struct ConnectQuery {
    pub ticket: Option<String>,
}

/// `GET /api/pty` — list all PTY sessions (group-pty.ts `pty.list`).
///
/// Returns `Location.response([Pty.Info])` = `{ location, data: [Info…] }`.
pub async fn list(
    State(state): State<SharedState>,
    _loc: Query<LocationQuery>,
) -> Json<Value> {
    let infos = manager().list();
    location_response(&state, infos)
}

/// `POST /api/pty` — create a PTY session (group-pty.ts `pty.create`).
///
/// Spawns via [`PtyManager::create`] and returns the new `Pty.Info` wrapped
/// in `Location.response` with status `201 Created`. A spawn failure yields 500.
pub async fn create(
    State(state): State<SharedState>,
    _loc: Query<LocationQuery>,
    Json(body): Json<CreateBody>,
) -> Response {
    let input = CreateInput {
        command: body.command,
        args: body.args,
        cwd: body.cwd,
        title: body.title,
        env: body.env,
        size: body.size.map(|s| PtySizeInput { rows: s.rows, cols: s.cols }),
    };
    match manager().create(input) {
        Ok(id) => {
            let info = manager().get(&id);
            if let Some(info) = info {
                (StatusCode::CREATED, location_response(&state, info)).into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "_tag": "UnknownError", "message": "pty created but vanished" })),
                )
                    .into_response()
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "_tag": "UnknownError", "message": e.to_string() })),
        )
            .into_response(),
    }
}

/// `GET /api/pty/:ptyID` — get one PTY session (group-pty.ts `pty.get`).
///
/// Returns `Location.response(Pty.Info)` or 404 `PtyNotFoundError`.
pub async fn get(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    _loc: Query<LocationQuery>,
) -> Response {
    match manager().get(&id) {
        Some(info) => location_response(&state, info).into_response(),
        None => pty_not_found(&id),
    }
}

/// `PUT /api/pty/:ptyID` — update title / size (group-pty.ts `pty.update`).
///
/// Applies `title` and/or `size` from `Pty.UpdateInput` via
/// [`PtyManager::set_title`] / [`PtyManager::update_size`], then returns
/// `Location.response(Pty.Info)`. 404 `PtyNotFoundError` for unknown id.
pub async fn update(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    _loc: Query<LocationQuery>,
    Json(body): Json<UpdateBody>,
) -> Response {
    if manager().get(&id).is_none() {
        return pty_not_found(&id);
    }
    if let Some(size) = body.size {
        if let Err(e) = manager().update_size(&id, size.rows, size.cols) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "_tag": "UnknownError", "message": e.to_string() })),
            )
                .into_response();
        }
    }
    if let Some(title) = body.title {
        if let Err(e) = manager().set_title(&id, &title) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "_tag": "UnknownError", "message": e.to_string() })),
            )
                .into_response();
        }
    }
    match manager().get(&id) {
        Some(info) => location_response(&state, info).into_response(),
        None => pty_not_found(&id),
    }
}

/// `DELETE /api/pty/:ptyID` — remove a PTY session (group-pty.ts `pty.remove`).
///
/// Kills the child and drops the session. Returns `204 No Content`
/// (HttpApiSchema.NoContent) or 404 `PtyNotFoundError` for unknown id.
pub async fn remove(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    _loc: Query<LocationQuery>,
) -> Response {
    if manager().remove(&id) {
        // Emit pty.deleted event (pty.ts:37) before responding.
        crate::state::emit(&state, "pty.deleted", json!({ "id": id }));
        StatusCode::NO_CONTENT.into_response()
    } else {
        pty_not_found(&id)
    }
}

/// `POST /api/pty/:ptyID/connect-token` — issue a single-use ticket
/// (group-pty.ts `pty.connectToken`).
///
/// Mints a `tkt_*` ticket via [`new_pty_ticket`], stores it in
/// [`AppState::pty_tickets`] scoped to this `ptyID`, and returns
/// `Location.response(PtyTicket.ConnectToken)` = `{location, data:{ticket,expires_in}}`.
/// The ticket is consumed exactly once by [`connect`]. 404 for unknown id.
pub async fn connect_token(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    _loc: Query<LocationQuery>,
) -> Response {
    if manager().get(&id).is_none() {
        return pty_not_found(&id);
    }
    let ticket = new_pty_ticket();
    state.pty_tickets.write().insert(
        ticket.clone(),
        PtyTicket {
            pty_id: id,
            created_at: chrono::Utc::now().timestamp_millis(),
        },
    );
    location_response(
        &state,
        json!({ "ticket": ticket, "expires_in": TICKET_EXPIRES_IN }),
    )
    .into_response()
}

/// `GET /api/pty/:ptyID/connect` — WebSocket connect (group-pty.ts
/// `pty.connect`).
///
/// Validates the ticket before upgrading. Two auth paths (group-pty.ts):
///   1. **Header** `x-opencode-ticket: 1` — trusted internal caller (origin
///      validation is handled by the CORS middleware).
///   2. **Query** `?ticket=tkt_…` — must match a stored single-use ticket for
///      this `ptyID`; consumed on success.
///
/// Returns `404` (unknown pty) or `403` (invalid ticket) before the upgrade.
/// After upgrading, the WS handler replays buffered output via
/// `chunk_replay` + `meta_frame(cursor)`, streams live output, and decodes
/// inbound input via `decode_input`.
pub async fn connect(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(query): Query<ConnectQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    // (a) Existence check → 404 PtyNotFoundError before upgrade (group-pty.ts).
    if manager().get(&id).is_none() {
        return Err(pty_not_found(&id));
    }

    // (b) Ticket validation: header (trusted) OR query (single-use ticket).
    let header_ok = headers
        .get(TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v == TOKEN_HEADER_VALUE)
        .unwrap_or(false);

    let query_ok = if let Some(ticket) = &query.ticket {
        // Single-use: remove and verify it matched this ptyID.
        let entry = state.pty_tickets.write().remove(ticket);
        entry.is_some_and(|t| t.pty_id == id)
    } else {
        false
    };

    if !header_ok && !query_ok {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "_tag": "ForbiddenError",
                "message": "invalid or missing PTY connect ticket",
            })),
        )
            .into_response());
    }

    // (c) Upgrade — the callback runs on the established WebSocket.
    Ok(ws.on_upgrade(move |socket| handle_ws(socket, id)))
}

// ────────────────────── WebSocket session loop ────────────────────
//
// Mirrors the opencode pty-protocol.ts wire format:
//   * replay buffered output via chunk_replay (bounded REPLAY_CHUNK frames)
//   * send meta_frame(cursor) so the client can resume on reconnect
//   * stream live output (drain_output polls every ~50 ms)
//   * decode inbound frames via decode_input and forward to the PTY stdin
//   * close the socket when the child exits

async fn handle_ws(mut socket: WebSocket, id: String) {
    // (1) Replay: drain the initial buffer, send bounded chunks, then the
    // meta frame carrying the absolute output cursor.
    let initial = manager().drain_output(&id);
    let mut cursor: u64 = initial.len() as u64;
    if !initial.is_empty() {
        let text = String::from_utf8_lossy(&initial);
        for chunk in loom_pty_protocol::chunk_replay(&text) {
            if socket.send(Message::Text(chunk.to_string())).await.is_err() {
                return;
            }
        }
    }
    // Always send the cursor meta frame (even for cursor=0) so the client
    // knows replay is complete and can resume from this point later.
    if socket
        .send(Message::Binary(loom_pty_protocol::meta_frame(cursor)))
        .await
        .is_err()
    {
        return;
    }

    // (2) Live stream + input loop.
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            // Inbound: decode and forward to PTY stdin.
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if let Some(decoded) = loom_pty_protocol::decode_input(t.as_bytes()) {
                            let _ = manager().write_input(&id, decoded.as_bytes());
                        }
                    }
                    Some(Ok(Message::Binary(b))) => {
                        if let Some(decoded) = loom_pty_protocol::decode_input(&b) {
                            let _ = manager().write_input(&id, decoded.as_bytes());
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {} // ping/pong handled automatically
                }
            }
            // Outbound: poll for new output.
            _ = ticker.tick() => {
                let output = manager().drain_output(&id);
                if !output.is_empty() {
                    cursor += output.len() as u64;
                    let text = String::from_utf8_lossy(&output);
                    if socket.send(Message::Text(text.into_owned())).await.is_err() {
                        break;
                    }
                }
                // Detect child exit → send final cursor + close.
                match manager().get(&id) {
                    Some(info) if info.status == "exited" => {
                        let _ = socket
                            .send(Message::Binary(loom_pty_protocol::meta_frame(cursor)))
                            .await;
                        let _ = socket.close().await;
                        break;
                    }
                    None => break, // session removed
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Header/query constant strings must match group-pty.ts exactly.
    #[test]
    fn connect_constants_match_contract() {
        assert_eq!(TICKET_QUERY, "ticket");
        assert_eq!(TOKEN_HEADER, "x-opencode-ticket");
        assert_eq!(TOKEN_HEADER_VALUE, "1");
    }

    /// `CreateBody` / `UpdateBody` must deserialize from the schema-pty.ts
    /// shapes (all-optional fields).
    #[test]
    fn bodies_match_schema_shapes() {
        let create: CreateBody =
            serde_json::from_str(r#"{"command":"bash","args":["-l"],"size":{"rows":30,"cols":120}}"#)
                .unwrap();
        assert_eq!(create.command.as_deref(), Some("bash"));
        assert_eq!(create.args.as_deref(), Some(&["-l".to_string()][..]));
        assert_eq!(create.size.unwrap().rows, 30);

        let empty: CreateBody = serde_json::from_str("{}").unwrap();
        assert!(empty.command.is_none());

        let update: UpdateBody = serde_json::from_str(r#"{"title":"renamed"}"#).unwrap();
        assert_eq!(update.title.as_deref(), Some("renamed"));
        assert!(update.size.is_none());
    }
}
