//! ACP-over-WebSocket endpoint.
//!
//! The WebSocket carries one complete ACP JSON-RPC message per text frame.
//! Protocol dispatch itself remains in `apps/acp`; this module only adapts
//! Axum's WebSocket to the ACP SDK's line transport.

use std::path::PathBuf;
use std::process::Stdio;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    http::{header::ORIGIN, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

use crate::state::SharedState;

const MAX_ACP_WS_MESSAGE_BYTES: usize = 1024 * 1024;

/// Upgrade an authenticated HTTP request to an ACP JSON-RPC WebSocket.
pub async fn connect(
    State(state): State<SharedState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_allowed(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    // `SharedState` is currently unused here: each ACP WebSocket connection
    // spawns its own `loom acp` subprocess instead of borrowing the durable
    // `AcpHub` agent. The `State` extractor is kept so the route signature
    // and middleware chain remain unchanged.
    let _ = state;
    ws.max_message_size(MAX_ACP_WS_MESSAGE_BYTES)
        .max_frame_size(MAX_ACP_WS_MESSAGE_BYTES)
        .on_upgrade(handle_socket)
}

/// Browsers always send Origin on a WebSocket upgrade. Native CLI clients do
/// not, and are authenticated by the normal server middleware. Remote browser
/// origins must be explicitly configured instead of inheriting permissive CORS.
fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) else {
        return true;
    };
    if let Ok(configured) = std::env::var("LOOM_ACP_ALLOWED_ORIGINS") {
        return configured
            .split(',')
            .map(str::trim)
            .any(|allowed| allowed == origin);
    }
    origin.starts_with("http://localhost:")
        || origin.starts_with("https://localhost:")
        || origin.starts_with("http://127.0.0.1:")
        || origin.starts_with("https://127.0.0.1:")
        || origin.starts_with("http://[::1]:")
        || origin.starts_with("https://[::1]:")
}

/// Locate the `loom` binary used to spawn the ACP child process.
///
/// Resolution order:
///   1. `LOOM_ACP_BINARY` environment variable (full path).
///   2. `loom` / `loom.exe` next to the running `loom-server` binary.
///   3. Bare `loom` (PATH lookup).
fn loom_binary() -> PathBuf {
    if let Ok(value) = std::env::var("LOOM_ACP_BINARY") {
        return PathBuf::from(value);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["loom", "loom.exe"] {
                let candidate = dir.join(name);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }
    PathBuf::from("loom")
}

async fn handle_socket(mut socket: WebSocket) {
    let mut child = match Command::new(loom_binary())
        .arg("acp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            tracing::error!(error = %err, "failed to spawn `loom acp` for ACP WebSocket bridge");
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };

    let child_stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            tracing::error!("`loom acp` child is missing a stdin pipe");
            return;
        }
    };
    let child_stdout = match child.stdout.take() {
        Some(stdout) => BufReader::new(stdout),
        None => {
            tracing::error!("`loom acp` child is missing a stdout pipe");
            return;
        }
    };

    let (mut ws_sink, mut ws_stream) = socket.split();

    // WS text frame -> child stdin (newline-terminated JSON line).
    let mut ws_to_child = tokio::spawn(async move {
        let mut stdin = child_stdin;
        while let Some(frame) = ws_stream.next().await {
            match frame {
                Ok(Message::Text(text)) => {
                    let mut line = text.to_string();
                    while matches!(line.as_bytes().last(), Some(b'\n') | Some(b'\r')) {
                        line.pop();
                    }
                    if line.is_empty() {
                        continue;
                    }
                    if stdin.write_all(line.as_bytes()).await.is_err()
                        || stdin.write_all(b"\n").await.is_err()
                    {
                        break;
                    }
                    // ACP expects each JSON-RPC request on its own line; flush
                    // so the child does not stall on buffered bytes.
                    if stdin.flush().await.is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) => break,
                Ok(Message::Ping(_) | Message::Pong(_)) => continue,
                Ok(Message::Binary(_)) => continue,
                Err(err) => {
                    tracing::debug!(error = %err, "ACP WebSocket read error");
                    break;
                }
            }
        }
        // Dropping `stdin` closes the child's stdin; the child sees EOF and exits.
    });

    // child stdout lines -> WS text frames.
    let mut child_to_ws = tokio::spawn(async move {
        let mut lines = child_stdout.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.is_empty() {
                continue;
            }
            if ws_sink.send(Message::Text(line)).await.is_err() {
                break;
            }
        }
        let _ = ws_sink.send(Message::Close(None)).await;
    });

    // Whichever side finishes first tears the bridge down; `child.wait()` is
    // a safety net if the child dies before either task notices.
    let child_status = tokio::select! {
        _ = &mut ws_to_child => None,
        _ = &mut child_to_ws => None,
        status = child.wait() => Some(status),
    };
    ws_to_child.abort();
    child_to_ws.abort();
    if let Some(Ok(status)) = child_status {
        if !status.success() {
            tracing::warn!(?status, "`loom acp` exited non-zero");
        }
    }
    let _ = child.wait().await;
}

#[allow(dead_code)]
fn disconnect_cancels() -> bool {
    std::env::var("LOOM_ACP_DISCONNECT_POLICY")
        .map(|value| value.eq_ignore_ascii_case("cancel"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_native_clients_are_allowed_by_default() {
        assert!(origin_allowed(&HeaderMap::new()));
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        assert!(origin_allowed(&headers));
    }

    #[test]
    fn remote_browser_origin_is_rejected_without_allowlist() {
        std::env::remove_var("LOOM_ACP_ALLOWED_ORIGINS");
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "https://untrusted.example".parse().unwrap());
        assert!(!origin_allowed(&headers));
    }

    #[test]
    fn disconnect_policy_defaults_to_persist() {
        std::env::remove_var("LOOM_ACP_DISCONNECT_POLICY");
        assert!(!disconnect_cancels());
    }
}
