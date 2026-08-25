//! SSE (Server-Sent Events) stream consumer for anureo-server.
//!
//! anureo-server exposes two SSE channels:
//!
//! | Path | Version | Envelope |
//! |------|---------|----------|
//! | `GET /global/event` | v1 | `{"directory", "payload": {"type", "properties"}}` |
//! | `GET /api/event` | v2 | Full `GlobalEvent` with `project?`, `workspace?`, `payload.id` |
//!
//! # Connection lifecycle
//!
//! 1. **T+0s**: `server.connected` event with `version` in properties.
//! 2. **T+10s**: `server.heartbeat` business event (every 10 s).
//! 3. **Live events**: workload events (`message.updated`, `session.status`, etc.).
//! 4. **Keepalive**: `keepalive` text line every 10 s (TCP-level).
//!
//! # Reconnection
//!
//! `SseStream` implements exponential back-off reconnect. After a disconnect,
//! it waits `base_delay * 2^attempt` (capped at 30s) before retrying, up to
//! `max_attempts`. The cursor (last event id) is tracked automatically.

use std::collections::VecDeque;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use core::future::Future;
use futures_util::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::Sleep;

use super::error::{TransportError, TransportResult};
use super::http::HttpTransport;

/// SSE channel selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SseChannelKind {
    /// `GET /global/event` — v1 flat envelope.
    V1,
    /// `GET /api/event` — v2 full envelope with project/workspace.
    #[default]
    V2,
}

impl SseChannelKind {
    /// Returns the SSE endpoint path for this channel.
    pub fn path(&self) -> &'static str {
        match self {
            SseChannelKind::V1 => "/global/event",
            SseChannelKind::V2 => "/api/event",
        }
    }
}

/// A parsed SSE event yielded by [`SseStream`].
#[derive(Debug, Clone)]
pub struct SseEvent {
    /// The raw event data string from the SSE `data:` line.
    pub raw: String,
    /// Parsed envelope. Shape depends on the channel version.
    pub envelope: SseEnvelope,
    /// The SSE `event:` field value.
    pub event_type: Option<String>,
}

impl SseEvent {
    /// Returns the business event type string (e.g. `"message.updated"`).
    pub fn event_type_str(&self) -> &str {
        match &self.envelope {
            SseEnvelope::V1(v1) => v1.payload.r#type.as_deref().unwrap_or(""),
            SseEnvelope::V2(v2) => v2.payload.event_type.as_deref().unwrap_or(""),
        }
    }

    /// Returns `true` for `server.connected` and `server.heartbeat`.
    pub fn is_system(&self) -> bool {
        matches!(
            self.event_type_str(),
            "server.connected" | "server.heartbeat"
        )
    }

    /// Returns `true` for the `keepalive` text line.
    pub fn is_keepalive(&self) -> bool {
        self.raw == "keepalive"
    }

    /// Returns the event id (v2 only; `None` for v1 and keepalive).
    pub fn event_id(&self) -> Option<&str> {
        match &self.envelope {
            SseEnvelope::V2(v2) => v2.payload.event_id.as_deref(),
            _ => None,
        }
    }

    /// Returns the `properties` JSON value from the envelope.
    pub fn properties(&self) -> &Value {
        match &self.envelope {
            SseEnvelope::V1(v1) => &v1.payload.properties,
            SseEnvelope::V2(v2) => &v2.payload.properties,
        }
    }
}

/// Parsed SSE envelope. The v1 and v2 shapes share semantic fields.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SseEnvelope {
    /// v1: `{"directory", "payload": {"type", "properties"}}`
    V1(SseEnvelopeV1),
    /// v2: `{"directory", "project?", "workspace?", "payload": {"id", "type", "properties"}}`
    V2(SseEnvelopeV2),
}

/// v1 SSE envelope (from `GET /global/event`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SseEnvelopeV1 {
    pub directory: String,
    pub payload: SsePayloadV1,
}

/// v1 payload: minimal type + properties.
#[derive(Debug, Clone, Deserialize)]
pub struct SsePayloadV1 {
    #[serde(rename = "type", default)]
    pub r#type: Option<String>,
    pub properties: Value,
}

/// v2 SSE envelope (from `GET /api/event`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SseEnvelopeV2 {
    pub directory: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub payload: SsePayloadV2,
}

/// v2 payload: includes event id.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsePayloadV2 {
    #[serde(rename = "id", default)]
    pub event_id: Option<String>,
    #[serde(rename = "type", default)]
    pub event_type: Option<String>,
    pub properties: Value,
}

// ─── Shared state between reader task and Stream poller ─────────────────────

/// Thread-safe shared state used to communicate between the background
/// SSE reader task and the `Stream::poll_next` implementation.
#[derive(Default)]
struct Shared {
    /// Buffered events ready to be yielded.
    buffer: VecDeque<SseEvent>,
    /// Whether the SSE connection has been closed.
    closed: bool,
    /// Fatal error that caused the connection to close, if any.
    error: Option<TransportError>,
    /// Last event id seen (for cursor tracking).
    last_event_id: Option<String>,
    /// Number of reconnect attempts made.
    attempts: usize,
    /// Stored waker from the current `poll_next` call.
    waker: Option<Waker>,
    /// Set to true by the reader when it adds an event to the buffer.
    event_available: bool,
}

// ─── SseStream ───────────────────────────────────────────────────────────────

/// An async `Stream` of parsed SSE events from anureo-server.
///
/// Construct via [`SseStream::builder`]:
///
/// ```ignore
/// let stream = SseStream::builder(http_transport, SseChannelKind::V2)
///     .with_reconnect(true)
///     .build();
/// tokio::pin!(stream);
/// while let Some(result) = stream.next().await {
///     let event = result?;
///     dbg!(event.event_type_str());
/// }
/// ```
///
/// # Architecture
///
/// The stream manages a background task that reads SSE events from an HTTP
/// endpoint and pushes them into a shared buffer. The `Stream::poll_next`
/// implementation drains that buffer and handles reconnection back-off.
pub struct SseStream {
    http: HttpTransport,
    channel: SseChannelKind,
    reconnect: bool,
    max_attempts: usize,
    base_delay: Duration,
    filter_system: bool,
    shared: Arc<tokio::sync::Mutex<Shared>>,
    /// Cancel signal sent to the reader task.
    cancel_tx: Option<mpsc::Sender<()>>,
    /// Timer for reconnect back-off.
    reconnect_timer: Option<Pin<Box<Sleep>>>,
    /// True when waiting for the reconnect timer to fire.
    reconnect_pending: bool,
}

impl fmt::Debug for SseStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SseStream")
            .field("channel", &self.channel)
            .field("reconnect", &self.reconnect)
            .field("base_delay", &self.base_delay)
            .field(
                "attempts",
                &self.shared.try_lock().map(|g| g.attempts).unwrap_or(0),
            )
            .finish()
    }
}

impl SseStream {
    /// Start building an SSE stream.
    pub fn builder(http: HttpTransport, channel: SseChannelKind) -> SseStreamBuilder {
        SseStreamBuilder {
            http,
            channel,
            reconnect: true,
            max_attempts: 5,
            base_delay: Duration::from_secs(1),
            cursor: None,
            filter_system: true,
        }
    }

    /// Build a non-reconnecting (single-shot) stream.
    pub fn new(http: HttpTransport, channel: SseChannelKind) -> Self {
        Self::builder(http, channel).with_reconnect(false).build()
    }

    /// Returns the last seen event id (cursor), if any.
    pub async fn cursor(&self) -> Option<String> {
        self.shared.lock().await.last_event_id.clone()
    }

    /// Returns `true` if the stream is currently in a reconnect back-off wait.
    pub fn is_reconnecting(&self) -> bool {
        self.reconnect_pending
    }

    /// Returns the number of reconnect attempts made so far.
    pub async fn attempts(&self) -> usize {
        self.shared.lock().await.attempts
    }

    /// Returns `true` if all reconnect attempts have been exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.reconnect_pending
            && self
                .shared
                .try_lock()
                .map(|g| g.attempts >= self.max_attempts)
                .unwrap_or(false)
    }
}

/// Builder for [`SseStream`].
#[derive(Debug)]
pub struct SseStreamBuilder {
    http: HttpTransport,
    channel: SseChannelKind,
    reconnect: bool,
    max_attempts: usize,
    base_delay: Duration,
    cursor: Option<String>,
    filter_system: bool,
}

impl SseStreamBuilder {
    /// Enable or disable automatic reconnection. Default: `true`.
    pub fn with_reconnect(mut self, reconnect: bool) -> Self {
        self.reconnect = reconnect;
        self
    }

    /// Maximum reconnect attempts. Default: 5.
    pub fn max_attempts(mut self, n: usize) -> Self {
        self.max_attempts = n;
        self
    }

    /// Base delay for exponential back-off. Default: 1 s, capped at 30 s.
    pub fn base_delay(mut self, delay: Duration) -> Self {
        self.base_delay = delay;
        self
    }

    /// Initial cursor (event id) for replay on reconnect.
    pub fn with_cursor(mut self, cursor: String) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// Disable system-event filtering. Default: filter on.
    pub fn with_no_filter(mut self) -> Self {
        self.filter_system = false;
        self
    }

    /// Build the [`SseStream`].
    pub fn build(self) -> SseStream {
        let shared = Arc::new(tokio::sync::Mutex::new(Shared {
            last_event_id: self.cursor.clone(),
            ..Default::default()
        }));
        let (cancel_tx, _cancel_rx) = mpsc::channel(1);

        SseStream {
            http: self.http,
            channel: self.channel,
            reconnect: self.reconnect,
            max_attempts: self.max_attempts,
            base_delay: self.base_delay,
            filter_system: self.filter_system,
            shared,
            cancel_tx: Some(cancel_tx),
            reconnect_timer: None,
            reconnect_pending: false,
        }
    }
}

// ─── Stream implementation ───────────────────────────────────────────────────

impl Stream for SseStream {
    type Item = TransportResult<SseEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        // ─── Reconnect timer ─────────────────────────────────────────
        if let Some(timer) = this.reconnect_timer.as_mut() {
            if Pin::new(timer).poll(cx).is_ready() {
                this.reconnect_timer = None;
                this.reconnect_pending = false;
            } else {
                return Poll::Pending;
            }
        }

        // ─── Drain shared buffer ─────────────────────────────────────
        let item = {
            let mut guard = match this.shared.try_lock() {
                Ok(g) => g,
                Err(_) => return Poll::Pending,
            };

            // Store waker for the reader task to use
            guard.waker = Some(cx.waker().clone());

            // Try to pop an event (filtering as needed)
            while let Some(ev) = guard.buffer.pop_front() {
                // Update cursor
                if let Some(id) = ev.event_id() {
                    guard.last_event_id = Some(id.to_string());
                }

                if this.filter_system && (ev.is_keepalive() || ev.is_system()) {
                    continue; // Skip filtered event
                }

                guard.event_available = !guard.buffer.is_empty();
                return Poll::Ready(Some(Ok(ev)));
            }

            // ─── Check terminal state ───────────────────────────────
            if guard.closed {
                guard.event_available = false;
                if let Some(err) = guard.error.take() {
                    return Poll::Ready(Some(Err(err)));
                }
                return Poll::Ready(None);
            }

            // Buffer is empty and stream is open — store waker and wait
            guard.event_available = false;
            None
        };

        if item.is_some() {
            return Poll::Ready(item);
        }

        // ─── Start reader if not pending ─────────────────────────────
        if this.reconnect_timer.is_none() && !this.reconnect_pending {
            this.start_reader();
        }

        Poll::Pending
    }
}

impl SseStream {
    /// Spawn the background task that reads SSE events and pushes them into `shared`.
    fn start_reader(&mut self) {
        // Cancel any existing reader
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.try_send(());
        }

        let url = self.http.make_url(self.channel.path());
        let auth_value = self.http.auth_value().cloned();
        let client = self.http.client().clone();
        let base_url = self.http.url().clone();
        let path = self.channel.path().to_string();
        let channel = self.channel;
        let shared = Arc::clone(&self.shared);

        let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);
        self.cancel_tx = Some(cancel_tx);

        tokio::spawn(async move {
            tokio::select! {
                biased;

                _ = cancel_rx.recv() => {
                    // Cancelled (new reader started)
                }
                result = read_sse_stream(
                    &url,
                    auth_value.as_deref(),
                    client,
                    &base_url,
                    &path,
                    channel,
                    Arc::clone(&shared),
                ) => {
                    let mut guard = shared.lock().await;
                    match result {
                        Ok(()) => {
                            guard.closed = true;
                        }
                        Err(e) => {
                            tracing::debug!(error = %e, "SSE stream error");
                            guard.error = Some(e);
                            guard.closed = true;
                        }
                    }
                    guard.event_available = true;
                    if let Some(w) = guard.waker.take() {
                        w.wake();
                    }
                }
            }
        });

        // Increment attempts counter
        let _attempts = self
            .shared
            .try_lock()
            .map(|mut g| {
                g.attempts += 1;
                g.attempts
            })
            .unwrap_or(1);
    }
}

// ─── SSE reader task ─────────────────────────────────────────────────────────

/// Reads SSE events from the given URL and pushes them into `shared`.
async fn read_sse_stream(
    url: &reqwest::Url,
    auth_value: Option<&str>,
    client: reqwest::Client,
    base_url: &reqwest::Url,
    path: &str,
    channel: SseChannelKind,
    shared: Arc<tokio::sync::Mutex<Shared>>,
) -> TransportResult<()> {
    let mut req = client
        .get(url.clone())
        .header("Accept", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive");

    // Auth value is stored as-is (either a raw token or a full
    // Authorization header value). No transformation is applied.
    if let Some(value) = auth_value {
        req = req.header(reqwest::header::AUTHORIZATION, value);
    }

    let response = req
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(TransportError::from)?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.bytes().await.unwrap_or_default().to_vec();
        return Err(TransportError::HttpError {
            status,
            base_url: base_url.to_string(),
            path: path.to_string(),
            body: String::from_utf8_lossy(&body).to_string(),
        });
    }

    let mut stream = response.bytes_stream();
    let mut line_buf = Vec::with_capacity(256);
    let mut data_accumulator = String::new();
    let mut current_event_type: Option<String> = None;

    while let Some(chunk_result) = stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(_) => break,
        };

        for &byte in chunk.as_ref() {
            if byte == b'\n' {
                let line = String::from_utf8_lossy(&line_buf).trim().to_string();
                line_buf.clear();

                if line.is_empty() {
                    if !data_accumulator.is_empty() {
                        let raw = std::mem::take(&mut data_accumulator);
                        data_accumulator = String::new();

                        let envelope = if raw == "keepalive" {
                            SseEnvelope::V1(SseEnvelopeV1 {
                                directory: String::new(),
                                payload: SsePayloadV1 {
                                    r#type: Some("keepalive".to_string()),
                                    properties: Value::Null,
                                },
                            })
                        } else {
                            let parsed: Value = serde_json::from_str(&raw)
                                .map_err(TransportError::ResponseParseError)?;
                            parse_envelope_from_value(&parsed, channel)
                        };

                        let event = SseEvent {
                            raw,
                            envelope,
                            event_type: current_event_type.take(),
                        };

                        // Push into shared buffer and wake the poller
                        let mut guard = shared.lock().await;
                        guard.buffer.push_back(event);
                        guard.event_available = true;
                        if let Some(w) = guard.waker.as_ref() {
                            w.wake_by_ref();
                        }
                    }
                    continue;
                }

                if let Some((field, value)) = line.split_once(':') {
                    let field = field.trim();
                    let value = value.trim();
                    match field {
                        "data" => {
                            if !data_accumulator.is_empty() {
                                data_accumulator.push('\n');
                            }
                            data_accumulator.push_str(value);
                        }
                        "event" => {
                            current_event_type = Some(value.to_string());
                        }
                        "id" | "retry" => {}
                        _ => {}
                    }
                }
            } else if byte != b'\r' {
                line_buf.push(byte);
            }
        }
    }

    Ok(())
}

/// Parse the envelope based on channel version.
fn parse_envelope_from_value(value: &Value, channel: SseChannelKind) -> SseEnvelope {
    match channel {
        SseChannelKind::V2 => serde_json::from_value(value.clone())
            .map(SseEnvelope::V2)
            .unwrap_or_else(|_| fallback_envelope(value)),
        SseChannelKind::V1 => serde_json::from_value(value.clone())
            .map(SseEnvelope::V1)
            .unwrap_or_else(|_| fallback_envelope(value)),
    }
}

/// Fallback envelope when JSON parsing fails.
fn fallback_envelope(value: &Value) -> SseEnvelope {
    SseEnvelope::V1(SseEnvelopeV1 {
        directory: value
            .get("directory")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string(),
        payload: SsePayloadV1 {
            r#type: value
                .get("payload")
                .and_then(|p| p.get("type"))
                .and_then(|t| t.as_str())
                .map(String::from),
            properties: value
                .get("payload")
                .and_then(|p| p.get("properties"))
                .cloned()
                .unwrap_or(Value::Null),
        },
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_v1_envelope() {
        let json = serde_json::json!({
            "directory": "/home/user/project",
            "payload": {
                "type": "message.updated",
                "properties": {"sessionID": "sess_1"}
            }
        });
        let envelope = parse_envelope_from_value(&json, SseChannelKind::V1);
        match envelope {
            SseEnvelope::V1(v1) => {
                assert_eq!(v1.directory, "/home/user/project");
                assert_eq!(v1.payload.r#type.as_deref(), Some("message.updated"));
            }
            _ => panic!("expected V1"),
        }
    }

    #[test]
    fn test_parse_v2_envelope() {
        let json = serde_json::json!({
            "directory": "/home/user/project",
            "project": "proj_abc",
            "workspace": "ws_xyz",
            "payload": {
                "id": "evt_1",
                "type": "session.status",
                "properties": {"sessionID": "sess_1"}
            }
        });
        let envelope = parse_envelope_from_value(&json, SseChannelKind::V2);
        match envelope {
            SseEnvelope::V2(v2) => {
                assert_eq!(v2.directory, "/home/user/project");
                assert_eq!(v2.project.as_deref(), Some("proj_abc"));
                assert_eq!(v2.payload.event_id.as_deref(), Some("evt_1"));
            }
            _ => panic!("expected V2"),
        }
    }

    #[test]
    fn test_parse_v1_json_direct() {
        let json_str =
            r#"{"directory":"/tmp","payload":{"type":"server.connected","properties":{}}}"#;
        let value: Value = serde_json::from_str(json_str).unwrap();
        let envelope: SseEnvelope = serde_json::from_value(value).unwrap();
        assert!(matches!(envelope, SseEnvelope::V1(_)));
    }

    #[test]
    fn test_parse_v2_json_direct() {
        let json_str = r#"{"directory":"/tmp","project":"p1","payload":{"id":"e1","type":"server.heartbeat","properties":{}}}"#;
        let value: Value = serde_json::from_str(json_str).unwrap();
        let envelope = parse_envelope_from_value(&value, SseChannelKind::V2);
        assert!(matches!(envelope, SseEnvelope::V2(_)));
    }

    #[test]
    fn test_fallback_envelope() {
        let bad_json = serde_json::json!({"not": "valid envelope"});
        let envelope = fallback_envelope(&bad_json);
        match envelope {
            SseEnvelope::V1(v1) => assert_eq!(v1.directory, ""),
            _ => panic!("expected V1 fallback"),
        }
    }

    #[test]
    fn test_sse_event_system_detection() {
        let json = serde_json::json!({
            "directory": "/tmp",
            "payload": {"type": "server.connected", "properties": {}}
        });
        let event = SseEvent {
            raw: json.to_string(),
            envelope: serde_json::from_value(json).unwrap(),
            event_type: None,
        };
        assert!(event.is_system());
        assert!(!event.is_keepalive());
    }

    #[test]
    fn test_sse_event_keepalive() {
        let event = SseEvent {
            raw: "keepalive".to_string(),
            envelope: SseEnvelope::V1(SseEnvelopeV1 {
                directory: String::new(),
                payload: SsePayloadV1 {
                    r#type: Some("keepalive".to_string()),
                    properties: Value::Null,
                },
            }),
            event_type: None,
        };
        assert!(event.is_keepalive());
        assert!(!event.is_system());
    }

    #[test]
    fn test_sse_event_properties() {
        let json = serde_json::json!({
            "directory": "/tmp",
            "payload": {"type": "message.updated", "properties": {"foo": "bar"}}
        });
        let event = SseEvent {
            raw: json.to_string(),
            envelope: serde_json::from_value(json).unwrap(),
            event_type: None,
        };
        assert_eq!(
            event.properties().get("foo").and_then(|v| v.as_str()),
            Some("bar")
        );
    }

    #[test]
    fn test_channel_path() {
        assert_eq!(SseChannelKind::V1.path(), "/global/event");
        assert_eq!(SseChannelKind::V2.path(), "/api/event");
    }

    #[test]
    fn test_channel_default() {
        assert_eq!(SseChannelKind::default(), SseChannelKind::V2);
    }

    #[test]
    fn test_shared_default() {
        let shared = Shared::default();
        assert!(!shared.closed);
        assert!(shared.error.is_none());
        assert!(!shared.event_available);
        assert!(shared.buffer.is_empty());
    }
}
