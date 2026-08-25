//! Typed error mapping for the anureo-server transport layer.
//!
//! All errors are wrapped in [`TransportError`] which distinguishes:
//!
//! - **Network errors** — connection failures, timeouts, DNS resolution.
//! - **HTTP errors** — anureo-server returned a non-2xx status. The raw body
//!   is captured so callers can deserialize server-side error details.
//! - **SSE errors** — malformed event lines, WebSocket handshake failures.
//! - **JSON errors** — request/response serialization failures.
//! - **Timeout** — the operation exceeded its deadline.
//! - **Cancelled** — the operation was explicitly cancelled.

use std::io;
use std::time::Duration;

use thiserror::Error;

/// Unified result type for the transport layer.
pub type TransportResult<T> = Result<T, TransportError>;

/// Unified error type for the transport layer.
#[derive(Error, Debug)]
pub enum TransportError {
    // ─── Network layer ────────────────────────────────────────────────
    /// Failed to establish a TCP connection.
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    /// TLS handshake or certificate validation failed.
    #[error("TLS error: {0}")]
    TlsFailed(String),

    // ─── HTTP layer ───────────────────────────────────────────────────
    /// anureo-server returned a non-2xx HTTP status.
    #[error("HTTP {status} from {base_url}{path}: {body}")]
    HttpError {
        status: u16,
        base_url: String,
        path: String,
        body: String,
    },

    /// HTTP response body exceeded the configured size limit.
    #[error("response body too large (max {max_bytes} bytes)")]
    BodyTooLarge { max_bytes: usize },

    // ─── SSE / WebSocket layer ────────────────────────────────────────
    /// SSE event line could not be parsed.
    #[error("SSE parse error: {0}")]
    SseParseError(String),

    /// SSE channel closed unexpectedly.
    #[error("SSE stream closed unexpectedly")]
    SseStreamClosed,

    /// WebSocket handshake with anureo-server failed.
    #[error("WebSocket handshake failed: {0}")]
    WebSocketFailed(String),

    // ─── JSON / Serde layer ───────────────────────────────────────────
    /// Request body serialization failed.
    #[error("failed to serialize request body: {0}")]
    RequestSerError(#[source] serde_json::Error),

    /// Response body could not be parsed as JSON.
    #[error("failed to parse response as JSON: {0}")]
    ResponseParseError(#[source] serde_json::Error),

    // ─── Timeout / Cancellation ───────────────────────────────────────
    /// The operation exceeded its deadline.
    #[error("request timed out after {timeout:?}")]
    Timeout { timeout: Duration },

    /// The operation was cancelled via a `CancellationToken`.
    #[error("operation cancelled")]
    Cancelled,

    // ─── Session / API logic ──────────────────────────────────────────
    /// Session not found (404).
    #[error("session not found: {0}")]
    SessionNotFound(String),

    /// Session is in an invalid state for the requested operation.
    #[error("invalid session state: {reason}")]
    InvalidSessionState { reason: String },

    /// anureo-server returned an explicit error in the response body.
    #[error("server error: {message}")]
    ServerError { message: String },

    /// The server returned 204 No Content (or an empty body on 2xx) but the
    /// caller expected a typed JSON response.
    ///
    /// This variant distinguishes "the operation succeeded with no body" from
    /// "the connection succeeded but the JSON was malformed." Callers who
    /// intentionally use a no-body endpoint (e.g. `delete`, `abort`) should
    /// use the typed `no_body` variant of the transport instead.
    #[error("server returned empty response for {path} (expected JSON body)")]
    EmptySuccess { path: String },
}

impl TransportError {
    /// Returns `true` if the error is retryable (transient network issues).
    ///
    /// HTTP 5xx, timeouts, and connection failures are retryable.
    /// 4xx errors, session-not-found, and parse errors are not.
    pub fn is_retryable(&self) -> bool {
        match self.http_status() {
            Some(s) => s >= 500,
            None => matches!(
                self,
                TransportError::ConnectionFailed(_) | TransportError::Timeout { .. }
            ),
        }
    }

    /// Returns the HTTP status code if this is an HTTP error, else `None`.
    pub fn http_status(&self) -> Option<u16> {
        match self {
            TransportError::HttpError { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Returns `true` if the error indicates the session does not exist.
    pub fn is_session_not_found(&self) -> bool {
        matches!(self, TransportError::SessionNotFound(_))
    }
}

// ─── reqwest integration ────────────────────────────────────────────────────

impl From<reqwest::Error> for TransportError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            return TransportError::Timeout {
                timeout: Duration::from_secs(30), // default; reqwest doesn't expose exact timeout
            };
        }
        if err.is_connect() {
            if let Some(url) = err.url() {
                return TransportError::ConnectionFailed(format!("{}: {}", url, err));
            }
            return TransportError::ConnectionFailed(err.to_string());
        }
        if err.is_decode() {
            return TransportError::ResponseParseError(
                serde_json::from_str::<serde_json::Value>(&err.to_string()).unwrap_err(),
            );
        }
        // Fall back: wrap whatever is left as a connection error
        TransportError::ConnectionFailed(err.to_string())
    }
}

// ─── tokio-tungstenite integration ─────────────────────────────────────────

impl From<tokio_tungstenite::tungstenite::Error> for TransportError {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        use tokio_tungstenite::tungstenite::Error as WsErr;
        match err {
            WsErr::ConnectionClosed => TransportError::SseStreamClosed,
            WsErr::AlreadyClosed => TransportError::SseStreamClosed,
            WsErr::Io(err) => TransportError::ConnectionFailed(err.to_string()),
            WsErr::Tls(err) => TransportError::TlsFailed(err.to_string()),
            WsErr::Url(err) => TransportError::WebSocketFailed(format!("invalid URL: {err}")),
            WsErr::Http(resp) => TransportError::HttpError {
                status: resp.status().as_u16(),
                base_url: String::new(),
                path: String::new(),
                body: String::new(),
            },
            WsErr::Capacity(_) => {
                TransportError::WebSocketFailed("WebSocket capacity exceeded".to_string())
            }
            _ => TransportError::WebSocketFailed(err.to_string()),
        }
    }
}

// ─── std::io::Error integration ─────────────────────────────────────────────

impl From<io::Error> for TransportError {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::TimedOut => TransportError::Timeout {
                timeout: Duration::from_secs(30),
            },
            _ => TransportError::ConnectionFailed(err.to_string()),
        }
    }
}

// ─── serde_json::Error integration ──────────────────────────────────────────

impl From<serde_json::Error> for TransportError {
    fn from(err: serde_json::Error) -> Self {
        // Heuristic: if the error mentions "key" or "number" it's usually a response parse.
        let msg = err.to_string();
        if msg.contains("key") || msg.contains("number") || msg.contains("expect") {
            TransportError::ResponseParseError(err)
        } else {
            TransportError::RequestSerError(err)
        }
    }
}
