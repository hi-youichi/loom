//! JSON-RPC 2.0 plumbing for the e2e harness.
//!
//! Plan 026 §2.1.3: loom-acp exchanges newline-delimited JSON-RPC 2.0 frames
//! over stdio. A frame is one of:
//!
//! ```text
//! { "jsonrpc": "2.0", "id": N, "method": "...", "params": {...} }   request (Client → Agent)
//! { "jsonrpc": "2.0", "id": N, "result": ... }                       response
//! { "jsonrpc": "2.0", "id": N, "error": {...} }                      error response
//! { "jsonrpc": "2.0", "method": "...", "params": {...} }             notification (no id)
//! { "jsonrpc": "2.0", "id": N, "method": "...", "params": {...} }   reverse RPC (Agent → Client)
//! ```
//!
//! Schema keys are accessed as `serde_json::Value` paths so the harness
//! tolerates `agent-client-protocol` 0.15.x → 0.16.x additions without code
//! changes (plan 026 Appendix D risk #1).

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{Mutex, Notify};

/// One parsed JSON-RPC frame.
#[derive(Debug, Clone)]
pub struct JsonRpcFrame {
    pub raw: Value,
}

impl JsonRpcFrame {
    pub fn id(&self) -> Option<u64> {
        self.raw.get("id").and_then(Value::as_u64)
    }

    pub fn method(&self) -> Option<&str> {
        self.raw.get("method").and_then(Value::as_str)
    }

    pub fn is_response(&self) -> bool {
        self.raw.get("result").is_some() || self.raw.get("error").is_some()
    }

    pub fn is_request(&self) -> bool {
        self.id().is_some() && self.method().is_some() && !self.is_response()
    }

    pub fn is_notification(&self) -> bool {
        self.method().is_some() && self.id().is_none() && !self.is_response()
    }
}

/// Lightweight session notification wrapper — we keep it as `Value` so
/// typed assertions can grow incrementally.
#[derive(Debug, Clone)]
pub struct SessionNotification {
    pub method: String,
    pub params: Value,
}

/// Categorized reverse-RPC call (Agent → Client). Used by `permissions.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReverseRpcKind {
    RequestPermission,
    FsReadTextFile,
    FsWriteTextFile,
    TerminalCreate,
    TerminalOutput,
    TerminalKill,
    ExtMethod(String),
}

impl ReverseRpcKind {
    pub fn classify(method: &str) -> Self {
        match method {
            "session/request_permission" => Self::RequestPermission,
            "fs/read_text_file" => Self::FsReadTextFile,
            "fs/write_text_file" => Self::FsWriteTextFile,
            "terminal/create" => Self::TerminalCreate,
            "terminal/output" => Self::TerminalOutput,
            "terminal/kill" => Self::TerminalKill,
            other => Self::ExtMethod(other.to_string()),
        }
    }
}

/// Pending-response slot shared between the writer (request side)
/// and the background reader (response side).
#[derive(Default)]
pub struct PendingResponses {
    pub map: HashMap<u64, Arc<Notify>>,
    /// Stores the full response value so `request()` can return it.
    pub values: HashMap<u64, Value>,
}

/// JSON-RPC client over a duplex byte stream.
///
/// Concurrency model:
/// - `next_id` is bumped atomically per request.
/// - The background reader task classifies incoming frames and either signals
///   a pending `Notify` (storing the response value) or pushes to the
///   notification queue.
/// - `request()` returns once its `id`'s `Notify` fires.
pub struct JsonRpcClient {
    next_id: AtomicU64,
    pending: Arc<Mutex<PendingResponses>>,
    notifications: Arc<Mutex<Vec<SessionNotification>>>,
}

impl JsonRpcClient {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            pending: Arc::new(Mutex::new(PendingResponses::default())),
            notifications: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn pending_handle(&self) -> Arc<Mutex<PendingResponses>> {
        Arc::clone(&self.pending)
    }

    pub fn notifications_handle(&self) -> Arc<Mutex<Vec<SessionNotification>>> {
        Arc::clone(&self.notifications)
    }

    pub fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    pub async fn register_pending(&self, id: u64) -> Arc<Notify> {
        let notify = Arc::new(Notify::new());
        self.pending
            .lock()
            .await
            .map
            .insert(id, Arc::clone(&notify));
        notify
    }

    /// Pop the stored response value for `id` (called after `Notify` fires).
    pub async fn take_response(&self, id: u64) -> Option<Value> {
        self.pending.lock().await.values.remove(&id)
    }

    pub async fn drain_notifications(&self) -> Vec<SessionNotification> {
        let mut q = self.notifications.lock().await;
        std::mem::take(&mut *q)
    }

    /// Drain notifications, keeping only those for which `pred` returns false.
    /// Returns the removed (matching) notifications.
    pub async fn drain_matching<F>(&self, pred: F) -> Vec<SessionNotification>
    where
        F: Fn(&SessionNotification) -> bool,
    {
        let mut q = self.notifications.lock().await;
        let (matching, remaining): (Vec<_>, Vec<_>) = q.drain(..).partition(|n| pred(n));
        *q = remaining;
        matching
    }

    pub fn build_request(&self, method: &str, params: Value) -> (u64, Value) {
        let id = self.next_request_id();
        let frame = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        (id, frame)
    }

    pub fn build_notification(&self, method: &str, params: Value) -> Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        })
    }

    /// Build a JSON-RPC **response** frame (for answering reverse RPCs).
    pub fn build_response(id: u64, result: Value) -> Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        })
    }
}

impl Default for JsonRpcClient {
    fn default() -> Self {
        Self::new()
    }
}
