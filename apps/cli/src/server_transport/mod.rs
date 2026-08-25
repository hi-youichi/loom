//! HTTP/SSE transport layer for communicating with anureo-server.
//!
//! Provides a typed, reusable client for the anureo-server REST API and
//! Server-Sent Events (SSE) channels. Designed for CLI integration where
//! the `anureo` binary connects to a running `anureo-server` instance.
//!
//! # Core types
//!
//! - [`AnureoServerClient`] — high-level client combining HTTP and SSE.
//! - [`HttpTransport`] — low-level HTTP primitives (auth, request, response).
//! - [`SseStream`] — SSE event stream with cursor tracking and reconnect.
//! - [`TransportError`] — unified error type covering all failure modes.
//!
//! # Session API
//!
//! The session CRUD and run endpoints map directly to anureo-server routes:
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | `POST` | `/session` | Create a new session |
//! | `GET`  | `/session/:id` | Fetch session metadata |
//! | `DELETE` | `/session/:id` | Delete a session |
//! | `POST` | `/session/:id/prompt` | Run a synchronous prompt |
//! | `POST` | `/session/:id/prompt_async` | Fire-and-forget prompt |
//! | `POST` | `/session/:id/abort` | Abort the active run |
//! | `POST` | `/api/session/:id/agent` | v2 prompt alias |
//! | `POST` | `/api/session/:id/interrupt` | v2 abort alias |
//!
//! # SSE channels
//!
//! | Path | Version | Framing |
//! |------|---------|---------|
//! | `GET /global/event` | v1 | `{"directory", "payload": {"type", "properties"}}` |
//! | `GET /api/event` | v2 | Full `GlobalEvent` with `project?`, `workspace?`, `payload.id` |

mod client;
mod error;
mod http;
mod session;
mod sse;

pub use client::AnureoServerClient;
pub use error::{TransportError, TransportResult};
pub use http::HttpTransport;
pub use session::{PromptRequest, PromptResponse, SessionCreateRequest, SessionInfo};
pub use sse::{SseChannelKind, SseEvent, SseStream};

/// The SSE heartbeat interval published by anureo-server.
///
/// Both the v1 and v2 SSE handlers emit this via `KeepAlive::text("keepalive")`
/// as a TCP-level keepalive signal, independent of the `server.heartbeat`
/// business event.
pub const SSE_HEARTBEAT_INTERVAL_SECS: u64 = 10;
