//! ACP-over-WebSocket endpoint.
//!
//! Each WebSocket text frame carries one complete ACP JSON-RPC message.
//! The durable agent and session state live in [`AcpHub`]; this handler
//! attaches a WebSocket transport to it.  On disconnect the agent persists —
//! a new WebSocket connection resumes the session via `session/load`.

use std::time::Duration;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    http::{header::AUTHORIZATION, header::ORIGIN, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::acp_hub::{AcpHub, EventCursor, SessionOwner};
use crate::state::SharedState;

/// Max ACP WS message / frame size.
const MAX_MESSAGE_BYTES: usize = 1024 * 1024; // 1 MiB

/// Upgrade an HTTP request to an ACP JSON-RPC WebSocket.
pub async fn connect(
    State(state): State<SharedState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_allowed(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let owner = extract_owner(&headers);
    tracing::info!(principal = %owner.principal, "ACP WS upgrade request");
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_socket(state, owner, socket))
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

/// Extract the session owner from the Authorization header.
///
/// If `LOOM_AUTH_TOKEN` is set, the bearer token must match and the principal
/// is the token itself (truncated for display). If not set, the owner is
/// `local-anonymous`.
fn extract_owner(headers: &HeaderMap) -> SessionOwner {
    let Some(auth_header) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()) else {
        return SessionOwner::anonymous();
    };
    let token = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);
    if let Ok(expected) = std::env::var("LOOM_AUTH_TOKEN") {
        if token == expected {
            return SessionOwner::from_bearer(format!("token-{}", &token[..token.len().min(8)]));
        }
        tracing::warn!("ACP WS bearer token mismatch");
    }
    SessionOwner::anonymous()
}

/// Handle a single WebSocket connection.
async fn handle_socket(state: SharedState, owner: SessionOwner, socket: WebSocket) {
    let (ws_sink, ws_stream) = socket.split();

    // --- Incoming stream: WS text frames → io::Result<String> ---
    let (text_tx, text_rx) = mpsc::channel::<std::io::Result<String>>(64);
    tokio::spawn(async move {
        let mut ws_stream = ws_stream;
        while let Some(frame) = ws_stream.next().await {
            match frame {
                Ok(Message::Text(text)) => {
                    if text_tx.send(Ok(text)).await.is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) => break,
                Ok(Message::Ping(_) | Message::Pong(_)) => continue,
                Ok(Message::Binary(_)) => {
                    let _ = text_tx
                        .send(Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "binary frames not supported",
                        )))
                        .await;
                    break;
                }
                Err(e) => {
                    let _ = text_tx
                        .send(Err(std::io::Error::other(e.to_string())))
                        .await;
                    break;
                }
            }
        }
    });
    let incoming = tokio_stream::wrappers::ReceiverStream::new(text_rx);

    // --- Outgoing sink: String → WS text frames ---
    let outgoing = ws_sink
        .with(|line: String| async move { Ok::<_, axum::Error>(Message::Text(line)) })
        .sink_map_err(|e| std::io::Error::other(e.to_string()));

    let transport = agent_client_protocol::Lines::new(outgoing, incoming);

    // --- Attach to AcpHub ---
    let (agent, notification_rx, lease) = match state.acp_hub.attach_with(owner.clone(), None).await {
        Ok(triple) => triple,
        Err(e) => {
            tracing::error!(error = %e, principal = %owner.principal, "AcpHub attach failed");
            return;
        }
    };

    // --- Spawn ping/pong keep-alive task ---
    //
    // The axum WS sink is already consumed by the transport. We rely on
    // axum's built-in ping/pong at the TCP level and the client's
    // application-layer timeouts. If the client sends pings, the incoming
    // reader task already silently continues on Ping/Pong frames.

    // --- Run ACP dispatch ---
    let hub_clone = state.acp_hub.clone();
    let shutdown = async move {
        let _ = lease.await;
    };
    if let Err(e) = loom_acp::stdio_loop::run_agent_connection(
        agent,
        notification_rx,
        transport,
        shutdown,
    )
    .await
    {
        let err_str = format!("{:?}", e);
        if !err_str.contains("receiver dropped")
            && !err_str.contains("broken pipe")
            && !err_str.contains("unexpected eof")
        {
            tracing::error!(error = %err_str, "ACP WebSocket dispatch error");
        }
    }

    // Mark detachment for idle TTL tracking.
    hub_clone.note_detach().await;

    // Log connection stats.
    let stats = hub_clone.stats().await;
    tracing::info!(
        total_connections = stats.total_connections,
        total_reconnects = stats.total_reconnects,
        total_disconnects = stats.total_disconnects,
        replay_dropped = stats.total_replay_dropped,
        "ACP WS connection closed"
    );

    // Grace period for pending notifications to flush.
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    #[test]
    fn origin_and_auth_tests() {
        // All env-var-dependent tests in one function to avoid parallel races.

        // Default: local + native allowed
        std::env::remove_var("LOOM_ACP_ALLOWED_ORIGINS");
        assert!(origin_allowed(&HeaderMap::new()));
        let mut h = HeaderMap::new();
        h.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        assert!(origin_allowed(&h));

        // Remote browser rejected without allowlist
        let mut h = HeaderMap::new();
        h.insert(ORIGIN, "https://untrusted.example".parse().unwrap());
        assert!(!origin_allowed(&h));

        // Remote browser allowed via env
        std::env::set_var("LOOM_ACP_ALLOWED_ORIGINS", "https://trusted.example");
        let mut h = HeaderMap::new();
        h.insert(ORIGIN, "https://trusted.example".parse().unwrap());
        assert!(origin_allowed(&h));
        std::env::remove_var("LOOM_ACP_ALLOWED_ORIGINS");

        // Owner extraction: anonymous without auth
        std::env::remove_var("LOOM_AUTH_TOKEN");
        let owner = extract_owner(&HeaderMap::new());
        assert_eq!(owner.principal, "local-anonymous");

        // Owner extraction: from bearer when token configured
        std::env::set_var("LOOM_AUTH_TOKEN", "secret123");
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, "Bearer secret123".parse().unwrap());
        let owner = extract_owner(&h);
        assert!(owner.principal.starts_with("token-"));
        std::env::remove_var("LOOM_AUTH_TOKEN");
    }
}
