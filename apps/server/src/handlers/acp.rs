//! ACP-over-WebSocket endpoint.
//!
//! Each WebSocket text frame carries one complete ACP JSON-RPC message.
//! The durable agent and session state live in [`AcpHub`]; this handler
//! attaches a WebSocket transport to it.  On disconnect the agent persists —
//! a new WebSocket connection resumes the session via `session/load`.

use std::time::Duration;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{ConnectInfo, Extension, State},
    http::{header::AUTHORIZATION, header::ORIGIN, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::sync::mpsc;

use crate::acp_hub::SessionOwner;
use crate::auth::{
    attempt_ui_login, ui_password_configured, ui_session_token_valid, AcpAuthVerdict, LoginOutcome,
};
use crate::state::SharedState;

/// Max ACP WS message / frame size.
const MAX_MESSAGE_BYTES: usize = 1024 * 1024; // 1 MiB

/// JSON-RPC error code for "authentication required" in the `/acp` pre-auth
/// handshake. Pairs with `data.authRequired` so clients can distinguish a
/// locked gate from an unreachable server.
pub const AUTH_REQUIRED_ERROR_CODE: i64 = -32001;

/// Upgrade an HTTP request to an ACP JSON-RPC WebSocket.
pub async fn connect(
    State(state): State<SharedState>,
    Extension(verdict): Extension<AcpAuthVerdict>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_allowed(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let owner = extract_owner(&headers);
    tracing::info!(
        principal = %owner.principal,
        authenticated = verdict.0,
        "ACP WS upgrade request"
    );
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| {
            Box::pin(async move {
                if verdict.0 {
                    handle_socket(state, owner, socket).await
                } else {
                    handle_pre_auth_socket(state, peer.ip().to_string(), socket).await
                }
            })
        })
}

/// Pre-auth handshake for unauthenticated sockets.
///
/// The upgrade itself was allowed (browser WebSocket clients cannot observe
/// HTTP 401s on upgrades), so the gate lives in the protocol. Sockets loop
/// here until they present valid credentials:
///
/// - `initialize` → structured `-32001 { authRequired: true }` error (keeps
///   auth-state probes working without leaking protocol details).
/// - `_anureo.dev/auth/status` → gate configuration (password configured,
///   passkey availability). Read-only, safe pre-auth.
/// - `_anureo.dev/auth/login` → scrypt password verify + per-IP rate
///   limiting + JWT mint; success hands the socket to [`handle_socket`].
/// - `_anureo.dev/auth/authenticate` → session-token verify (tokens minted
///   here or Express cookies share one HS256 secret); success likewise
///   upgrades to [`handle_socket`].
///
/// Any other method closes the socket immediately. No ACP hub state is
/// touched until authentication succeeds.
async fn handle_pre_auth_socket(state: SharedState, peer_ip: String, mut socket: WebSocket) {
    loop {
        let first = tokio::time::timeout(PRE_AUTH_IDLE_TIMEOUT, socket.recv()).await;
        let text = match first {
            Ok(Some(Ok(Message::Text(text)))) => text,
            _ => {
                let _ = socket.send(Message::Close(None)).await;
                return;
            }
        };
        match pre_auth_dispatch(&peer_ip, &text).await {
            PreAuthOutcome::Respond(payload) => {
                if socket.send(Message::Text(payload)).await.is_err() {
                    return;
                }
            }
            PreAuthOutcome::RespondAndAuthenticate(payload) => {
                if socket.send(Message::Text(payload)).await.is_err() {
                    return;
                }
                tracing::info!(peer = %peer_ip, "ACP pre-auth socket authenticated");
                handle_socket(state, SessionOwner::anonymous(), socket).await;
                return;
            }
            PreAuthOutcome::Close => {
                let _ = socket.send(Message::Close(None)).await;
                return;
            }
        }
    }
}

/// Idle timeout for the pre-auth loop: covers both "connected but sent
/// nothing" and the single-probe pattern (probe clients close after the
/// first response anyway).
const PRE_AUTH_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
enum PreAuthOutcome {
    Respond(String),
    RespondAndAuthenticate(String),
    Close,
}

async fn pre_auth_dispatch(peer_ip: &str, text: &str) -> PreAuthOutcome {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return PreAuthOutcome::Close;
    };
    let id = value.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let method = value.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = value
        .get("params")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    match method {
        "initialize" => PreAuthOutcome::Respond(auth_error_response(&id)),
        "_anureo.dev/auth/status" => {
            let password_configured = ui_password_configured();
            PreAuthOutcome::Respond(
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "passwordConfigured": password_configured,
                        "passkeyEnabled": false,
                        "hasPasskeys": false,
                        "passkeyCount": 0,
                        "rpId": serde_json::Value::Null,
                    }
                })
                .to_string(),
            )
        }
        "_anureo.dev/auth/login" => {
            let password = params
                .get("password")
                .and_then(|p| p.as_str())
                .unwrap_or("");
            let trust_device = params
                .get("trustDevice")
                .and_then(|t| t.as_bool())
                .unwrap_or(false);
            let outcome = tokio::task::spawn_blocking({
                let password = password.to_string();
                let peer_ip = peer_ip.to_string();
                move || attempt_ui_login(&password, trust_device, &peer_ip)
            })
            .await
            .unwrap_or(LoginOutcome {
                ok: false,
                session_token: None,
                expires_at_unix: None,
                retry_after_secs: None,
            });
            if outcome.ok {
                // Token only — the socket itself stays pre-auth; clients
                // present the minted token via `authenticate` to upgrade.
                // (Login sockets are single-request probes that close on
                // response, so upgrading here would strand them.)
                PreAuthOutcome::Respond(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "authenticated": true,
                            "sessionToken": outcome.session_token,
                            "expiresAt": outcome.expires_at_unix,
                        }
                    })
                    .to_string(),
                )
            } else if let Some(retry_after) = outcome.retry_after_secs {
                PreAuthOutcome::Respond(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": 429,
                            "message": "Too many login attempts, please try again later",
                            "data": { "retryAfter": retry_after }
                        }
                    })
                    .to_string(),
                )
            } else {
                PreAuthOutcome::Respond(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": 401, "message": "Invalid credentials" }
                    })
                    .to_string(),
                )
            }
        }
        "_anureo.dev/auth/authenticate" => {
            let token = params
                .get("sessionToken")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if !token.is_empty() && ui_session_token_valid(token) {
                PreAuthOutcome::RespondAndAuthenticate(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "authenticated": true }
                    })
                    .to_string(),
                )
            } else {
                PreAuthOutcome::Respond(auth_error_response(&id))
            }
        }
        _ => PreAuthOutcome::Close,
    }
}

fn auth_error_response(id: &serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": AUTH_REQUIRED_ERROR_CODE,
            "message": "authentication required",
            "data": { "authRequired": true, "realm": "anureo-ui" }
        }
    })
    .to_string()
}

/// Browsers always send Origin on a WebSocket upgrade. Native CLI clients do
/// not, and are authenticated by the normal server middleware. Remote browser
/// origins must be explicitly configured instead of inheriting permissive CORS.
fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) else {
        return true;
    };
    if let Ok(configured) = std::env::var("ANUREO_ACP_ALLOWED_ORIGINS") {
        return configured
            .split(',')
            .map(str::trim)
            .any(|allowed| allowed == origin);
    }
    is_localhost_origin(origin)
}

/// Check whether `origin` is a localhost / 127.0.0.1 / [::1] URL with a
/// valid port number.  Uses `rsplit_once` on the host:port boundary so that
/// `http://127.0.0.1:3000` is correctly recognized while
/// `http://localhost:3000.evil.com` is rejected (the port segment is not a
/// valid u16).
fn is_localhost_origin(origin: &str) -> bool {
    let rest = match origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    {
        Some(r) => r,
        None => return false,
    };
    let (host, port_str) = rest.rsplit_once(':').unwrap_or((rest, ""));
    let port = port_str.split('/').next().unwrap_or(port_str);
    let port_ok = port.is_empty() || port.parse::<u16>().is_ok();
    port_ok && matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Extract the session owner from the Authorization header.
///
/// If `ANUREO_AUTH_TOKEN` is set, the bearer token must match and the principal
/// is the token itself (truncated for display). If not set, the owner is
/// `local-anonymous`.
fn extract_owner(headers: &HeaderMap) -> SessionOwner {
    let Some(auth_header) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()) else {
        return SessionOwner::anonymous();
    };
    let token = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);
    if let Ok(expected) = std::env::var("ANUREO_AUTH_TOKEN") {
        if token == expected {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            token.hash(&mut hasher);
            return SessionOwner::from_bearer(format!("token-{:016x}", hasher.finish()));
        }
        tracing::warn!("ACP WS bearer token mismatch");
    }
    SessionOwner::anonymous()
}

/// Handle a single WebSocket connection.
async fn handle_socket(state: SharedState, owner: SessionOwner, socket: WebSocket) {
    let (ws_sink, ws_stream) = socket.split();

    // Signal fired when the incoming reader task exits (WS closed / errored).
    // This lets the `shutdown` future resolve even without a lease cancel —
    // fixing the idle-disconnect hang where `connect_with` would otherwise
    // wait forever for the foreground (lease) to complete.
    let (ws_closed_tx, ws_closed_rx) = tokio::sync::oneshot::channel();

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
        // Signal that the WS has closed so `shutdown` can resolve.
        let _ = ws_closed_tx.send(());
    });
    let incoming = tokio_stream::wrappers::ReceiverStream::new(text_rx);

    // --- Outgoing sink: String → WS text frames ---
    let outgoing = ws_sink
        .with(|line: String| async move { Ok::<_, axum::Error>(Message::Text(line)) })
        .sink_map_err(|e| std::io::Error::other(e.to_string()));

    let transport = agent_client_protocol::Lines::new(outgoing, incoming);

    // --- Attach to AcpHub ---
    let lease = match state.acp_hub.attach_with(owner.clone(), None).await {
        Ok(lease) => lease,
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
    // Each WebSocket owns an independent ACP connection. Shutdown follows only
    // this socket; opening another connection never cancels it.
    let hub_clone = state.acp_hub.clone();
    let connection_id = lease.connection.id.clone();
    let shutdown = async move {
        let _ = ws_closed_rx.await;
    };
    if let Err(e) = anureo_acp::stdio_loop::run_agent_connection(
        lease.runtime,
        lease.connection,
        lease.outbound_rx,
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

    hub_clone.close_connection(&connection_id).await;

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
    fn auth_error_response_formats_initialize_error_with_echoed_id() {
        let response = auth_error_response(&serde_json::json!(42));
        let value: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["id"], 42);
        assert_eq!(value["error"]["code"], AUTH_REQUIRED_ERROR_CODE);
        assert_eq!(value["error"]["data"]["authRequired"], true);
    }

    #[tokio::test]
    async fn pre_auth_dispatch_routes_methods() {
        // Non-JSON and unknown methods close the socket.
        assert!(matches!(
            pre_auth_dispatch("127.0.0.1", "not json").await,
            PreAuthOutcome::Close
        ));
        assert!(matches!(
            pre_auth_dispatch(
                "127.0.0.1",
                r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#
            )
            .await,
            PreAuthOutcome::Close
        ));
        // `initialize` answers with the auth-required error.
        match pre_auth_dispatch(
            "127.0.0.1",
            r#"{"jsonrpc":"2.0","id":7,"method":"initialize","params":{}}"#,
        )
        .await
        {
            PreAuthOutcome::Respond(payload) => {
                let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
                assert_eq!(value["id"], 7);
                assert_eq!(value["error"]["code"], AUTH_REQUIRED_ERROR_CODE);
            }
            other => panic!("expected Respond, got {other:?}"),
        }
    }

    #[test]
    fn origin_and_auth_tests() {
        // All env-var-dependent tests in one function to avoid parallel races.

        // Default: local + native allowed
        std::env::remove_var("ANUREO_ACP_ALLOWED_ORIGINS");
        assert!(origin_allowed(&HeaderMap::new()));
        let mut h = HeaderMap::new();
        h.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        assert!(origin_allowed(&h));

        // Remote browser rejected without allowlist
        let mut h = HeaderMap::new();
        h.insert(ORIGIN, "https://untrusted.example".parse().unwrap());
        assert!(!origin_allowed(&h));

        // Remote browser allowed via env
        std::env::set_var("ANUREO_ACP_ALLOWED_ORIGINS", "https://trusted.example");
        let mut h = HeaderMap::new();
        h.insert(ORIGIN, "https://trusted.example".parse().unwrap());
        assert!(origin_allowed(&h));
        std::env::remove_var("ANUREO_ACP_ALLOWED_ORIGINS");

        // Owner extraction: anonymous without auth
        std::env::remove_var("ANUREO_AUTH_TOKEN");
        let owner = extract_owner(&HeaderMap::new());
        assert_eq!(owner.principal, "local-anonymous");

        // Owner extraction: from bearer when token configured
        std::env::set_var("ANUREO_AUTH_TOKEN", "secret123");
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, "Bearer secret123".parse().unwrap());
        let owner = extract_owner(&h);
        assert!(owner.principal.starts_with("token-"));
        std::env::remove_var("ANUREO_AUTH_TOKEN");
    }
}
