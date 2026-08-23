//! Ordered, resumable session-update streams for Loom Desk clients.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_client_protocol::schema::v1::SessionNotification;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::connection::ConnectionOutbound;
use crate::connection_registry::ConnectionRegistry;
use crate::session::SessionId;
use crate::session_bindings::SessionBindings;

pub const UPDATE_METHOD: &str = "_loomdesk.dev/session-sync/update";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSyncCursor {
    pub stream_id: String,
    pub seq: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSyncEvent {
    pub stream_id: String,
    pub seq: u64,
    pub event_id: String,
    pub emitted_at: u64,
    pub payload: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSyncOpenResult {
    pub mode: SessionSyncMode,
    pub session_id: String,
    pub stream_id: String,
    pub through_seq: u64,
    pub min_replay_seq: u64,
    pub prompt_state: SessionSyncPromptState,
    pub events: Vec<SessionSyncEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_reason: Option<SessionSyncResetReason>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionSyncPromptState {
    Idle,
    Running,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionSyncMode {
    Delta,
    ResetRequired,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionSyncResetReason {
    MissingCursor,
    StreamChanged,
    CursorAhead,
    ReplayWindowExceeded,
}

#[derive(Debug, Default)]
struct SessionSyncState {
    subscribers: HashMap<SessionId, HashSet<String>>,
}

#[derive(Debug)]
struct PersistedStream {
    stream_id: String,
    through_seq: u64,
    min_replay_seq: u64,
    events: Vec<SessionSyncEvent>,
}

#[derive(Debug)]
struct SessionSyncRepository {
    connection: Mutex<Connection>,
}

impl SessionSyncRepository {
    fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    #[cfg(test)]
    fn in_memory() -> rusqlite::Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> rusqlite::Result<Self> {
        connection.execute_batch(
            r#"
            PRAGMA busy_timeout=30000;
            CREATE TABLE IF NOT EXISTS acp_session_sync_streams (
                session_id TEXT PRIMARY KEY,
                stream_id TEXT NOT NULL,
                next_seq INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS acp_session_sync_events (
                session_id TEXT NOT NULL,
                stream_id TEXT NOT NULL,
                seq INTEGER NOT NULL,
                event_json TEXT NOT NULL,
                payload_bytes INTEGER NOT NULL,
                PRIMARY KEY(session_id, stream_id, seq)
            );
            CREATE INDEX IF NOT EXISTS idx_acp_session_sync_events_session_seq
                ON acp_session_sync_events(session_id, seq);
            "#,
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn ensure_stream(
        transaction: &rusqlite::Transaction<'_>,
        session_id: &SessionId,
    ) -> rusqlite::Result<(String, u64)> {
        if let Some(stream) = transaction
            .query_row(
                "SELECT stream_id, next_seq FROM acp_session_sync_streams WHERE session_id = ?1",
                [session_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
            )
            .optional()?
        {
            return Ok(stream);
        }
        let stream_id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO acp_session_sync_streams(session_id, stream_id, next_seq) VALUES (?1, ?2, 1)",
            params![session_id.to_string(), stream_id],
        )?;
        Ok((stream_id, 1))
    }

    fn append(
        &self,
        session_id: &SessionId,
        payload: Value,
        max_events: usize,
        max_bytes: usize,
    ) -> rusqlite::Result<Option<SessionSyncEvent>> {
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some((stream_id, next_seq)) = transaction
            .query_row(
                "SELECT stream_id, next_seq FROM acp_session_sync_streams WHERE session_id = ?1",
                [session_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let event = SessionSyncEvent {
            stream_id: stream_id.clone(),
            seq: next_seq,
            event_id: Uuid::new_v4().to_string(),
            emitted_at: now_millis(),
            payload,
        };
        let event_json = serde_json::to_string(&event)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let payload_bytes = event_json.len() as u64;
        transaction.execute(
            "INSERT INTO acp_session_sync_events(session_id, stream_id, seq, event_json, payload_bytes) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id.to_string(), stream_id, next_seq, event_json, payload_bytes],
        )?;
        transaction.execute(
            "UPDATE acp_session_sync_streams SET next_seq = ?2 WHERE session_id = ?1",
            params![session_id.to_string(), next_seq + 1],
        )?;
        let mut retained = transaction
            .prepare("SELECT seq, payload_bytes FROM acp_session_sync_events WHERE session_id = ?1 ORDER BY seq ASC")?
            .query_map([session_id.to_string()], |row| {
                Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut retained_bytes: u64 = retained.iter().map(|(_, bytes)| *bytes).sum();
        while retained.len() > max_events || retained_bytes > max_bytes as u64 {
            let (seq, bytes) = retained.remove(0);
            transaction.execute(
                "DELETE FROM acp_session_sync_events WHERE session_id = ?1 AND seq = ?2",
                params![session_id.to_string(), seq],
            )?;
            retained_bytes = retained_bytes.saturating_sub(bytes);
        }
        transaction.commit()?;
        Ok(Some(event))
    }

    fn read_or_create(&self, session_id: &SessionId) -> rusqlite::Result<PersistedStream> {
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (stream_id, next_seq) = Self::ensure_stream(&transaction, session_id)?;
        let events = transaction
            .prepare("SELECT event_json FROM acp_session_sync_events WHERE session_id = ?1 AND stream_id = ?2 ORDER BY seq ASC")?
            .query_map(params![session_id.to_string(), stream_id], |row| row.get::<_, String>(0))?
            .map(|row| {
                let raw = row?;
                serde_json::from_str(&raw).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        raw.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })
            .collect::<rusqlite::Result<Vec<SessionSyncEvent>>>()?;
        transaction.commit()?;
        Ok(PersistedStream {
            stream_id,
            through_seq: next_seq.saturating_sub(1),
            min_replay_seq: events.first().map(|event| event.seq).unwrap_or(next_seq),
            events,
        })
    }
}

/// Assigns a stable sequence to every live ACP session notification and keeps
/// a bounded replay window persisted beside the checkpoint database.
pub struct SessionSyncService {
    state: Mutex<SessionSyncState>,
    repository: SessionSyncRepository,
    connections: Arc<ConnectionRegistry>,
    bindings: Arc<SessionBindings>,
    max_events: usize,
    max_bytes: usize,
}

impl std::fmt::Debug for SessionSyncService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionSyncService")
            .field("max_events", &self.max_events)
            .field("max_bytes", &self.max_bytes)
            .finish_non_exhaustive()
    }
}

impl SessionSyncService {
    pub fn new(
        connections: Arc<ConnectionRegistry>,
        bindings: Arc<SessionBindings>,
        db_path: &Path,
    ) -> rusqlite::Result<Self> {
        Ok(Self::with_repository(
            connections,
            bindings,
            SessionSyncRepository::open(db_path)?,
            4_096,
            8 * 1024 * 1024,
        ))
    }

    #[cfg(test)]
    pub fn with_limits(
        connections: Arc<ConnectionRegistry>,
        bindings: Arc<SessionBindings>,
        max_events: usize,
        max_bytes: usize,
    ) -> Self {
        Self::with_repository(
            connections,
            bindings,
            SessionSyncRepository::in_memory().expect("in-memory session sync repository"),
            max_events,
            max_bytes,
        )
    }

    fn with_repository(
        connections: Arc<ConnectionRegistry>,
        bindings: Arc<SessionBindings>,
        repository: SessionSyncRepository,
        max_events: usize,
        max_bytes: usize,
    ) -> Self {
        Self {
            state: Mutex::new(SessionSyncState::default()),
            repository,
            connections,
            bindings,
            max_events: max_events.max(1),
            max_bytes: max_bytes.max(1),
        }
    }

    /// Record one canonical live update, then fan it out to attached sync
    /// subscribers. History replay must not call this method.
    pub async fn record(&self, notification: &SessionNotification) {
        let session_id = SessionId::new(notification.session_id.to_string());
        let payload = serde_json::json!({
            "type": "session_update",
            "update": notification.update,
        });
        let (event, subscribers) = {
            let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            let event = match self.repository.append(
                &session_id,
                payload,
                self.max_events,
                self.max_bytes,
            ) {
                Ok(Some(event)) => event,
                Ok(None) => return,
                Err(error) => {
                    tracing::error!(%error, %session_id, "failed to persist session sync event");
                    return;
                }
            };
            let subscribers = state
                .subscribers
                .get(&session_id)
                .cloned()
                .unwrap_or_default();
            (event, subscribers)
        };

        let params = serde_json::json!({
            "sessionId": session_id.to_string(),
            "streamId": event.stream_id,
            "events": [event],
        });
        for connection_id in subscribers {
            let Some(connection) = self.connections.get(&connection_id) else {
                continue;
            };
            if connection
                .outbound_tx
                .send(ConnectionOutbound::ExtensionNotification {
                    method: UPDATE_METHOD.to_string(),
                    params: params.clone(),
                })
                .await
                .is_err()
            {
                tracing::debug!(%connection_id, "session sync subscriber is no longer available");
            }
        }
    }

    /// Atomically captures the current high-water mark and subscribes the
    /// connection. Notifications may reach the transport before this request's
    /// response, so clients must buffer them until the open result is applied.
    pub async fn open(
        &self,
        session_id: SessionId,
        connection_id: String,
        cursor: Option<SessionSyncCursor>,
        prompt_state: SessionSyncPromptState,
    ) -> rusqlite::Result<SessionSyncOpenResult> {
        let result = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            let stream = self.repository.read_or_create(&session_id)?;
            let stream_id = stream.stream_id.clone();
            let through_seq = stream.through_seq;
            let min_replay_seq = stream.min_replay_seq;
            let (mode, events, reset_reason) = match cursor {
                None => (
                    SessionSyncMode::ResetRequired,
                    Vec::new(),
                    Some(SessionSyncResetReason::MissingCursor),
                ),
                Some(cursor) if cursor.stream_id != stream_id => (
                    SessionSyncMode::ResetRequired,
                    Vec::new(),
                    Some(SessionSyncResetReason::StreamChanged),
                ),
                Some(cursor) if cursor.seq > through_seq => (
                    SessionSyncMode::ResetRequired,
                    Vec::new(),
                    Some(SessionSyncResetReason::CursorAhead),
                ),
                Some(cursor) if cursor.seq.saturating_add(1) < min_replay_seq => (
                    SessionSyncMode::ResetRequired,
                    Vec::new(),
                    Some(SessionSyncResetReason::ReplayWindowExceeded),
                ),
                Some(cursor) => (
                    SessionSyncMode::Delta,
                    stream
                        .events
                        .iter()
                        .filter(|event| event.seq > cursor.seq)
                        .cloned()
                        .collect(),
                    None,
                ),
            };
            let result = SessionSyncOpenResult {
                mode,
                session_id: session_id.to_string(),
                stream_id,
                through_seq,
                min_replay_seq,
                prompt_state,
                events,
                reset_reason,
            };
            state
                .subscribers
                .entry(session_id.clone())
                .or_default()
                .insert(connection_id.clone());
            result
        };

        self.bindings
            .add_connection_to_session(&session_id, connection_id.clone());
        if let Some(connection) = self.connections.get(&connection_id) {
            connection.note_session(&session_id.to_string()).await;
        }
        Ok(result)
    }

    pub fn close_session(&self, session_id: &SessionId, connection_id: &str) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(subscribers) = state.subscribers.get_mut(session_id) {
            subscribers.remove(connection_id);
            if subscribers.is_empty() {
                state.subscribers.remove(session_id);
            }
        }
    }

    pub fn close_connection(&self, connection_id: &str) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.subscribers.retain(|_, subscribers| {
            subscribers.remove(connection_id);
            !subscribers.is_empty()
        });
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{SessionId as AcpSessionId, SessionUpdate};
    use tokio::sync::mpsc;

    fn fixture(
        max_events: usize,
    ) -> (
        SessionSyncService,
        Arc<crate::connection::AcpConnection>,
        mpsc::Receiver<ConnectionOutbound>,
    ) {
        let connections = Arc::new(ConnectionRegistry::default());
        let bindings = Arc::new(SessionBindings::new());
        let (tx, rx) = mpsc::channel(8);
        let connection = Arc::new(crate::connection::AcpConnection::shell(
            "connection-a".into(),
            "owner-a".into(),
            tx,
        ));
        connections.insert(connection.clone());
        (
            SessionSyncService::with_limits(connections, bindings, max_events, usize::MAX),
            connection,
            rx,
        )
    }

    fn notification(session_id: &str) -> SessionNotification {
        SessionNotification::new(
            AcpSessionId::new(session_id),
            SessionUpdate::AvailableCommandsUpdate(
                agent_client_protocol::schema::v1::AvailableCommandsUpdate::new(Vec::new()),
            ),
        )
    }

    #[tokio::test]
    async fn reconnect_replays_only_events_after_cursor() {
        let (service, connection, mut rx) = fixture(8);
        let session_id = SessionId::new("session-a");
        let initial = service
            .open(
                session_id.clone(),
                connection.id.clone(),
                None,
                SessionSyncPromptState::Idle,
            )
            .await
            .expect("initial open");
        assert_eq!(initial.mode, SessionSyncMode::ResetRequired);

        service.record(&notification("session-a")).await;
        let first = rx.recv().await.expect("live event");
        let ConnectionOutbound::ExtensionNotification { params, .. } = first else {
            panic!("expected extension notification");
        };
        assert_eq!(params["events"][0]["seq"], 1);
        service.record(&notification("session-a")).await;
        let _ = rx.recv().await.expect("second live event");

        let replay = service
            .open(
                session_id,
                connection.id.clone(),
                Some(SessionSyncCursor {
                    stream_id: initial.stream_id,
                    seq: 1,
                }),
                SessionSyncPromptState::Idle,
            )
            .await
            .expect("replay open");
        assert_eq!(replay.mode, SessionSyncMode::Delta);
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].seq, 2);
    }

    #[tokio::test]
    async fn disconnected_window_is_recorded_for_a_replacement_connection() {
        let (service, first, mut first_rx) = fixture(8);
        let session_id = SessionId::new("session-disconnected");
        let initial = service
            .open(
                session_id.clone(),
                first.id.clone(),
                None,
                SessionSyncPromptState::Running,
            )
            .await
            .expect("initial open");
        service.record(&notification("session-disconnected")).await;
        let _ = first_rx.recv().await.expect("first live event");
        service.close_connection(&first.id);

        service.record(&notification("session-disconnected")).await;
        let (tx, _rx) = mpsc::channel(8);
        let second = Arc::new(crate::connection::AcpConnection::shell(
            "connection-b".into(),
            "owner-a".into(),
            tx,
        ));
        service.connections.insert(second.clone());
        let replay = service
            .open(
                session_id,
                second.id.clone(),
                Some(SessionSyncCursor {
                    stream_id: initial.stream_id,
                    seq: 1,
                }),
                SessionSyncPromptState::Running,
            )
            .await
            .expect("replacement open");
        assert_eq!(replay.mode, SessionSyncMode::Delta);
        assert_eq!(replay.prompt_state, SessionSyncPromptState::Running);
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].seq, 2);
    }

    #[tokio::test]
    async fn evicted_cursor_requires_reset() {
        let (service, connection, _rx) = fixture(2);
        let session_id = SessionId::new("session-a");
        let initial = service
            .open(
                session_id.clone(),
                connection.id.clone(),
                None,
                SessionSyncPromptState::Idle,
            )
            .await
            .expect("initial open");
        for _ in 0..3 {
            service.record(&notification("session-a")).await;
        }

        let replay = service
            .open(
                session_id,
                connection.id.clone(),
                Some(SessionSyncCursor {
                    stream_id: initial.stream_id,
                    seq: 0,
                }),
                SessionSyncPromptState::Idle,
            )
            .await
            .expect("replay open");
        assert_eq!(replay.mode, SessionSyncMode::ResetRequired);
        assert_eq!(
            replay.reset_reason,
            Some(SessionSyncResetReason::ReplayWindowExceeded)
        );
    }

    #[tokio::test]
    async fn stream_and_replay_survive_service_restart() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("sync.db");
        let session_id = SessionId::new("session-restart");
        let stream_id = {
            let connections = Arc::new(ConnectionRegistry::default());
            let bindings = Arc::new(SessionBindings::new());
            let service =
                SessionSyncService::new(connections, bindings, &db_path).expect("first service");
            let initial = service
                .open(
                    session_id.clone(),
                    "connection-a".into(),
                    None,
                    SessionSyncPromptState::Idle,
                )
                .await
                .expect("initial open");
            service.record(&notification("session-restart")).await;
            initial.stream_id
        };

        let connections = Arc::new(ConnectionRegistry::default());
        let bindings = Arc::new(SessionBindings::new());
        let service =
            SessionSyncService::new(connections, bindings, &db_path).expect("restarted service");
        let replay = service
            .open(
                session_id,
                "connection-b".into(),
                Some(SessionSyncCursor {
                    stream_id: stream_id.clone(),
                    seq: 0,
                }),
                SessionSyncPromptState::Idle,
            )
            .await
            .expect("restart replay");
        assert_eq!(replay.mode, SessionSyncMode::Delta);
        assert_eq!(replay.stream_id, stream_id);
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].seq, 1);
    }

    #[test]
    fn failed_event_insert_rolls_back_sequence_allocation() {
        let repository = SessionSyncRepository::in_memory().expect("repository");
        let session_id = SessionId::new("session-rollback");
        repository.read_or_create(&session_id).expect("stream");
        repository
            .connection
            .lock()
            .expect("connection")
            .execute_batch(
                "CREATE TRIGGER fail_sync_insert BEFORE INSERT ON acp_session_sync_events BEGIN SELECT RAISE(ABORT, 'injected'); END;",
            )
            .expect("trigger");
        assert!(repository
            .append(&session_id, serde_json::json!({}), 8, usize::MAX)
            .is_err());
        repository
            .connection
            .lock()
            .expect("connection")
            .execute_batch("DROP TRIGGER fail_sync_insert;")
            .expect("drop trigger");
        let event = repository
            .append(&session_id, serde_json::json!({}), 8, usize::MAX)
            .expect("append")
            .expect("stream exists");
        assert_eq!(event.seq, 1);
    }
}
