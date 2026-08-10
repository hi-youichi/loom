//! OpenCode v2 event envelope and session-durable replay log.
//!
//! This module deliberately does not reuse `GlobalEvent`: legacy events use a
//! different cursor and payload contract.  V2 session events have a per-session
//! aggregate sequence and are the only events valid on `/api/session/:id/event`.

use std::collections::{HashMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

use crate::state::SharedState;

const V2_LOG_CAP: usize = 2_048;
static NEXT_V2_EVENT_ID: Mutex<u64> = Mutex::new(1);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct V2Durable {
    #[serde(rename = "aggregateID")]
    pub aggregate_id: String,
    pub seq: u64,
    pub version: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct V2Event {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durable: Option<V2Durable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Value>,
    pub data: Value,
}

/// Production append-only v2 durable log. Each session gets an independent
/// NDJSON file so replay never depends on the legacy global ring buffer.
pub struct V2FileLog {
    root: PathBuf,
}

impl V2FileLog {
    pub fn open(root: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn path(&self, session_id: &str) -> PathBuf {
        let safe: String = session_id
            .chars()
            .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
            .collect();
        self.root.join(format!("{safe}.jsonl"))
    }

    fn append(&self, event: &V2Event) -> std::io::Result<()> {
        let session_id = event
            .durable
            .as_ref()
            .expect("durable event")
            .aggregate_id
            .as_str();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path(session_id))?;
        serde_json::to_writer(&mut file, event).map_err(std::io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_data()
    }

    fn delete(&self, session_id: &str) -> std::io::Result<()> {
        match fs::remove_file(self.path(session_id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn load(&self) -> std::io::Result<HashMap<String, VecDeque<V2Event>>> {
        let mut logs = HashMap::new();
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("jsonl") {
                continue;
            }
            let file = fs::File::open(&path)?;
            let mut log = VecDeque::new();
            for line in BufReader::new(file).lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                let event: V2Event = serde_json::from_str(&line).map_err(std::io::Error::other)?;
                let Some(durable) = event.durable.as_ref() else {
                    continue;
                };
                if durable.aggregate_id != event.data["sessionID"].as_str().unwrap_or_default() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "v2 durable aggregate/session mismatch",
                    ));
                }
                if log
                    .back()
                    .and_then(|previous: &V2Event| previous.durable.as_ref())
                    .is_some_and(|previous| previous.seq >= durable.seq)
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "v2 durable sequence is not strictly increasing",
                    ));
                }
                log.push_back(event);
            }
            if let Some(session_id) = log
                .front()
                .and_then(|event| event.durable.as_ref())
                .map(|durable| durable.aggregate_id.clone())
            {
                logs.insert(session_id, log);
            }
        }
        Ok(logs)
    }
}

fn event_id() -> String {
    let mut next = NEXT_V2_EVENT_ID.lock();
    let id = *next;
    *next = next.wrapping_add(1);
    format!("evt_v2_{id}")
}

fn session_id(data: &Value) -> Option<&str> {
    data.get("sessionID").and_then(Value::as_str)
}

/// Effect `optional(...)` fields are absent on the wire, not JSON null. Keep
/// the typed v2 stream strict even though legacy Loom responses often retain
/// null placeholders for TUI compatibility.
fn strip_nulls(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.retain(|_, value| !value.is_null());
            for value in fields.values_mut() {
                strip_nulls(value);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(strip_nulls),
        _ => {}
    }
}

/// Append a session event before making it visible to SSE subscribers.
///
/// The current server store is in-memory, therefore the log is bounded.  The
/// separate state fields keep the public behaviour correct now and provide a
/// single seam for a persistent Store implementation.
pub fn publish_durable(
    state: &SharedState,
    event_type: impl Into<String>,
    mut data: Value,
    version: u32,
) -> Option<V2Event> {
    strip_nulls(&mut data);
    let session_id = session_id(&data)?.to_string();
    let _publish_guard = state.v2_publish_lock.lock();
    let seq = state
        .v2_next_seq
        .read()
        .get(&session_id)
        .copied()
        .unwrap_or(1);
    let event = {
        V2Event {
            id: event_id(),
            metadata: None,
            event_type: event_type.into(),
            durable: Some(V2Durable {
                aggregate_id: session_id.clone(),
                seq,
                version,
            }),
            location: None,
            data,
        }
    };

    // The store append precedes memory visibility and broadcast.  InMemory
    // storage cannot fail; a durable implementation must preserve this order.
    if let Some(store) = &state.store {
        store.append_v2_session_event(&event);
    }
    if let Some(file_log) = &state.v2_file_log {
        if let Err(error) = file_log.append(&event) {
            tracing::error!(%error, session_id, "failed to append v2 durable event");
            return None;
        }
    }
    let mut logs = state.v2_session_events.write();
    let log = logs.entry(session_id.clone()).or_default();
    if log.len() == V2_LOG_CAP {
        log.pop_front();
    }
    log.push_back(event.clone());
    drop(logs);
    state.v2_next_seq.write().insert(session_id, seq + 1);
    let _ = state.v2_event_tx.send(event.clone());
    Some(event)
}

/// Broadcast a v2 live-only event.  It intentionally has no durable cursor
/// and is never added to the session replay log.
pub fn publish_live(
    state: &SharedState,
    event_type: impl Into<String>,
    mut data: Value,
) -> V2Event {
    strip_nulls(&mut data);
    let event = V2Event {
        id: event_id(),
        metadata: None,
        event_type: event_type.into(),
        durable: None,
        location: None,
        data,
    };
    let _ = state.v2_event_tx.send(event.clone());
    event
}

pub fn replay_after(state: &SharedState, session_id: &str, after: u64) -> Vec<V2Event> {
    state
        .v2_session_events
        .read()
        .get(session_id)
        .map(|log| {
            log.iter()
                .filter(|event| event.durable.as_ref().is_some_and(|d| d.seq > after))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Highest sequence allocated for this session at a subscription boundary.
pub fn watermark(state: &SharedState, session_id: &str) -> u64 {
    state
        .v2_next_seq
        .read()
        .get(session_id)
        .copied()
        .unwrap_or(1)
        .saturating_sub(1)
}

pub fn subscribe(state: &SharedState) -> broadcast::Receiver<V2Event> {
    state.v2_event_tx.subscribe()
}

pub fn clear_session(state: &SharedState, session_id: &str) {
    let _publish_guard = state.v2_publish_lock.lock();
    if let Some(store) = &state.store {
        store.delete_v2_session_events(session_id);
    }
    if let Some(file_log) = &state.v2_file_log {
        if let Err(error) = file_log.delete(session_id) {
            tracing::error!(%error, session_id, "failed to delete v2 durable log");
        }
    }
    state.v2_session_events.write().remove(session_id);
    state.v2_next_seq.write().remove(session_id);
}

pub fn load_file_log(state: &SharedState) {
    let Some(file_log) = &state.v2_file_log else {
        return;
    };
    match file_log.load() {
        Ok(logs) => {
            let sequences = logs
                .iter()
                .map(|(session_id, log)| {
                    let next = log
                        .back()
                        .and_then(|event| event.durable.as_ref())
                        .map(|durable| durable.seq + 1)
                        .unwrap_or(1);
                    (session_id.clone(), next)
                })
                .collect();
            *state.v2_session_events.write() = logs;
            *state.v2_next_seq.write() = sequences;
        }
        Err(error) => {
            tracing::error!(%error, "failed to load v2 durable log; v2 replay disabled until repaired")
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::state::new_state;

    #[test]
    fn session_sequences_are_independent_and_exclusive() {
        let state = new_state();
        let first = publish_durable(
            &state,
            "session.next.text.started",
            json!({"sessionID":"sess_a"}),
            1,
        )
        .unwrap();
        let second = publish_durable(
            &state,
            "session.next.text.ended",
            json!({"sessionID":"sess_a"}),
            1,
        )
        .unwrap();
        let other = publish_durable(
            &state,
            "session.next.text.started",
            json!({"sessionID":"sess_b"}),
            1,
        )
        .unwrap();
        assert_eq!(first.durable.unwrap().seq, 1);
        assert_eq!(second.durable.unwrap().seq, 2);
        assert_eq!(other.durable.unwrap().seq, 1);
        assert_eq!(replay_after(&state, "sess_a", 1).len(), 1);
    }

    #[test]
    fn file_log_round_trips_and_deletes_a_session() {
        let root = std::env::temp_dir().join(format!("loom-v2-log-test-{}", uuid::Uuid::new_v4()));
        let log = V2FileLog::open(root.clone()).unwrap();
        let event = V2Event {
            id: "evt_v2_test".to_string(),
            metadata: None,
            event_type: "session.next.text.started".to_string(),
            durable: Some(V2Durable {
                aggregate_id: "sess_test".to_string(),
                seq: 1,
                version: 1,
            }),
            location: None,
            data: json!({"timestamp": 1, "sessionID": "sess_test", "assistantMessageID":"msg_test", "textID":"part_test"}),
        };
        log.append(&event).unwrap();
        assert_eq!(log.load().unwrap()["sess_test"].len(), 1);
        log.delete("sess_test").unwrap();
        assert!(log.load().unwrap().is_empty());
        std::fs::remove_dir(root).unwrap();
    }
}
