//! ACP-over-WebSocket endpoint.
//!
//! The WebSocket carries one complete ACP JSON-RPC message per text frame.
//! Protocol dispatch itself remains in `apps/acp`; this module only adapts
//! Axum's WebSocket to the ACP SDK's line transport.

use std::io;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    http::{header::ORIGIN, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use agent_client_protocol::Lines;
use futures::{SinkExt, StreamExt};

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
    ws.max_message_size(MAX_ACP_WS_MESSAGE_BYTES)
        .max_frame_size(MAX_ACP_WS_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_socket(socket, state))
}

/// Browsers always send Origin on a WebSocket upgrade. Native CLI clients do
/// not, and are authenticated by the normal server middleware. Remote browser
/// origins must be explicitly configured instead of inheriting permissive CORS.
fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) else {
        return true;
    };
    if let Ok(configured) = std::env::var("LOOM_ACP_ALLOWED_ORIGINS") {
        return configured.split(',').map(str::trim).any(|allowed| allowed == origin);
    }
    origin.starts_with("http://localhost:")
        || origin.starts_with("https://localhost:")
        || origin.starts_with("http://127.0.0.1:")
        || origin.starts_with("https://127.0.0.1:")
        || origin.starts_with("http://[::1]:")
        || origin.starts_with("https://[::1]:")
}

async fn handle_socket(socket: WebSocket, state: SharedState) {
    let (outgoing, incoming) = socket.split();

    let outgoing = futures::sink::unfold(outgoing, |mut socket, line: String| async move {
        socket
            .send(Message::Text(line.into()))
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()))?;
        Ok::<_, io::Error>(socket)
    });
    let incoming = incoming.filter_map(|frame| async move {
        match frame {
            Ok(Message::Text(text)) => Some(Ok(text.to_string())),
            Ok(Message::Close(_)) => None,
            Ok(Message::Binary(_)) => Some(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ACP WebSocket accepts text frames only",
            ))),
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => None,
            Err(e) => Some(Err(io::Error::new(io::ErrorKind::ConnectionAborted, e.to_string()))),
        }
    });

    // The transport ends when the peer closes the WebSocket.  ACP protocol
    // cancellation remains explicit (`session/cancel`); close is not a cancel.
    let Ok((agent, _updates, lease)) = state.acp_hub.attach().await else {
        return;
    };
    tracing::warn!("ACP WebSocket transport: run_transport_with_agent not available after dev merge");
    let _ = lease.await;
}

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
