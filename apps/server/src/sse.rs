//! SSE endpoints (task P0.4): `GET /global/event` (v1) + `GET /api/event` (v2).
//!
//! Both routes share the same broadcast bus (`AppState::event_tx`) but
//! use slightly different framing:
//!
//! - v1 `packages/tui/src/context/event.ts:12-19` expects a flat
//!   envelope `{ directory, payload: { type, properties } }`.
//! - v2 `packages/sdk/js/src/v2/gen/types.gen.ts:730-820` adds the
//!   optional `project?` + `workspace?` outer fields and a `payload.id`
//!   on every event. Both envelopes share the same upstream broadcast —
//!   one global sink, two serializers.
//!
//! Connection lifecycle:
//! - `T+0s`   : server.connected (business event with metadata)
//! - `T+10s`  : server.heartbeat (business event)
//! - Bus event: `<live>`  (real workload)
//! - `KeepAlive::comment` line every 10s for TCP-level keepalive
//!
//! The heartbeat is a *business* event (not just a comment line) so
//! TUI clients can recognize and log it; the comment line is for proxy
//! keepalive only — see `protocols/sse-events.md:32-56` + `external-kernel-guide.md:292-296`.

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::{Path, Query, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures::{stream, Stream, StreamExt};
use serde_json::json;
use tokio_stream::wrappers::{BroadcastStream, IntervalStream};

use crate::state::{GlobalEvent, SharedState};

/// Heartbeat interval — shared by business-event timer and TCP keepalive.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

/// v1 handler: `GET /global/event`.
pub async fn event_stream(
    State(state): State<SharedState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = build_stream(state, ChannelKind::V1);
    Sse::new(stream).keep_alive(keepalive())
}

/// v2 handler: `GET /api/event`.
pub async fn api_event_stream(
    State(state): State<SharedState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = build_stream(state, ChannelKind::V2);
    Sse::new(stream).keep_alive(keepalive())
}

/// v2 session-scoped handler: `GET /api/session/:id/event`.
pub async fn api_session_event_stream(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Query(query): Query<EventQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = build_session_stream(state, session_id, query.after);
    Sse::new(stream).keep_alive(keepalive())
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct EventQuery {
    #[serde(default)]
    pub after: Option<String>,
}

#[derive(Debug, Copy, Clone)]
pub enum ChannelKind {
    V1,
    V2,
}

fn build_stream(
    state: SharedState,
    kind: ChannelKind,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let rx = state.event_tx.subscribe();
    let bus = BroadcastStream::new(rx).filter_map(move |result| async move {
        let event = result.ok()?;
        serialize_event(&event, kind)
    });

    let connected = connection_event(
        &state,
        "server.connected",
        json!({
            "version": env!("CARGO_PKG_VERSION")
        }),
    );
    let seed = stream::once(async move { serialize_event(&connected, kind) })
        .filter_map(|event| async move { event });

    let heartbeat_state = state;
    let heartbeat = IntervalStream::new(tokio::time::interval_at(
        tokio::time::Instant::now() + HEARTBEAT_INTERVAL,
        HEARTBEAT_INTERVAL,
    ))
    .filter_map(move |_| {
        let event = connection_event(&heartbeat_state, "server.heartbeat", json!({}));
        async move { serialize_event(&event, kind) }
    });

    seed.chain(stream::select(bus, heartbeat))
}

fn build_session_stream(
    state: SharedState,
    session_id: String,
    after: Option<String>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let replay_session_id = session_id.clone();
    let replay = crate::state::snapshot_replay(&state, after.as_deref())
        .into_iter()
        .filter(move |event| event_matches_session(event, &replay_session_id))
        .filter_map(|event| serialize_event(&event, ChannelKind::V2));

    let live_session_id = session_id.clone();
    let rx = state.event_tx.subscribe();
    let bus = BroadcastStream::new(rx).filter_map(move |result| {
        let live_session_id = live_session_id.clone();
        async move {
            let event = result.ok()?;
            if !event_matches_session(&event, &live_session_id) {
                return None;
            }
            serialize_event(&event, ChannelKind::V2)
        }
    });

    let connected = connection_event(
        &state,
        "server.connected",
        json!({
            "sessionID": session_id,
            "version": env!("CARGO_PKG_VERSION")
        }),
    );
    let seed = stream::once(async move { serialize_event(&connected, ChannelKind::V2) })
        .filter_map(|event| async move { event });

    let heartbeat_state = state;
    let heartbeat_session_id = session_id;
    let heartbeat = IntervalStream::new(tokio::time::interval_at(
        tokio::time::Instant::now() + HEARTBEAT_INTERVAL,
        HEARTBEAT_INTERVAL,
    ))
    .filter_map(move |_| {
        let event = connection_event(
            &heartbeat_state,
            "server.heartbeat",
            json!({ "sessionID": heartbeat_session_id }),
        );
        async move { serialize_event(&event, ChannelKind::V2) }
    });

    seed.chain(stream::iter(replay)).chain(stream::select(bus, heartbeat))
}

fn event_matches_session(event: &GlobalEvent, session_id: &str) -> bool {
    event
        .payload
        .properties
        .get("sessionID")
        .and_then(serde_json::Value::as_str)
        == Some(session_id)
}

fn connection_event(
    state: &SharedState,
    event_type: &str,
    properties: serde_json::Value,
) -> GlobalEvent {
    let project = state.project.read();
    GlobalEvent::new(
        project.directory.clone(),
        Some(project.id.clone()),
        project.workspace_id.clone(),
        event_type.to_string(),
        properties,
    )
}

fn serialize_event(event: &GlobalEvent, kind: ChannelKind) -> Option<Result<Event, Infallible>> {
    match serialize(event, kind) {
        Ok(data) => Some(Ok(Event::default().data(data))),
        Err(error) => {
            tracing::warn!(%error, "failed to serialize SSE event");
            None
        }
    }
}

fn keepalive() -> KeepAlive {
    KeepAlive::new()
        .interval(HEARTBEAT_INTERVAL)
        .text("keepalive")
}

/// Per-channel event serializer.
fn serialize(ev: &GlobalEvent, kind: ChannelKind) -> Result<String, serde_json::Error> {
    match kind {
        ChannelKind::V1 => {
            // v1 TUI consumers (`packages/tui/src/context/event.ts`) only
            // inspect `directory + payload.type + payload.properties`. They
            // ignore the v2 fields when present, but we strip them so the
            // payload stays minimal on the wire.
            let v1 = serde_json::json!({
                "directory": ev.directory,
                "payload": {
                    "id": ev.payload.event_id,
                    "type": ev.payload.event_type,
                    "properties": ev.payload.properties,
                },
            });
            serde_json::to_string(&v1)
        }
        // V2 uses the custom Serialize impl on GlobalEvent which emits the
        // contract's flat EventSchema shape (schema/event.ts:54-61):
        //   { id, metadata?, type, durable?, location?, data }
        ChannelKind::V2 => serde_json::to_string(ev),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{emit, new_state};

    #[test]
    fn envelope_round_trips_v2() {
        let state = new_state();
        emit(&state, "test.event", json!({"hello": "world"}));
        let snap = crate::state::snapshot_replay(&state, None);
        assert_eq!(snap.len(), 1);
        let serialized_v2 = serialize(&snap[0], ChannelKind::V2).unwrap();
        // Flat shape: id + type at top level, data (not properties),
        // location:{directory,workspaceID?}, no payload wrapper, no top-level
        // directory/project/workspace.
        let parsed: serde_json::Value =
            serde_json::from_str(&serialized_v2).expect("valid JSON");
        assert_eq!(parsed["type"], "test.event");
        assert_eq!(parsed["data"]["hello"], "world");
        assert!(parsed["id"].as_str().unwrap().starts_with("evt_"));
        assert!(parsed["location"]["directory"].is_string());
        assert!(parsed.get("payload").is_none(), "no payload wrapper");
        assert!(parsed.get("properties").is_none(), "no properties field");
    }

    #[test]
    fn envelope_v1_drops_project_field() {
        let state = new_state();
        emit(&state, "test.event", json!({}));
        let snap = crate::state::snapshot_replay(&state, None);
        let serialized = serialize(&snap[0], ChannelKind::V1).unwrap();
        // V1 shape: { directory, payload: { type, properties } }.
        assert!(!serialized.contains("\"project\""));
        assert!(serialized.contains("\"directory\""));
        assert!(serialized.contains("\"payload\""));
        assert!(serialized.contains("test.event"));
    }

    #[test]
    fn snapshot_replay_filters_after_cursor() {
        let state = new_state();
        emit(&state, "a", json!({}));
        emit(&state, "b", json!({}));
        emit(&state, "c", json!({}));
        let snap = crate::state::snapshot_replay(&state, None);
        // Get the id of the second event:
        let second_id = &snap[1].payload.event_id;
        let after = crate::state::snapshot_replay(&state, Some(second_id));
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].payload.event_type, "c");
    }
}
