//! Per-connection state for ACP WebSocket / stdio.
//!
//! [`AcpConnection`] bundles all state tied to a single client connection
//! (WebSocket or stdio). It is created on `initialize` and bound to sessions
//! on `session/new` / `session/load`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agent_client_protocol::schema::v1::SessionNotification;
use tokio::sync::mpsc;

use crate::client_capabilities::ClientCapabilitiesInfo;
use crate::tools::ClientBridgeTrait;

/// Unique connection identifier (UUID string).
pub type ConnectionId = String;

/// Single WebSocket / stdio connection state.
///
/// Created in the `initialize` handler, bound to sessions via
/// [`crate::session::SessionStore::set_connection`], and deactivated
/// (`active = false`) when the transport closes.
pub struct AcpConnection {
    /// Unique ID for logging.
    pub id: ConnectionId,
    /// Authenticated principal.
    pub principal: String,
    /// Capabilities declared by the client in `initialize`.
    pub capabilities: ClientCapabilitiesInfo,
    /// Notification channel for pushing `session/update` to the client.
    pub notification_tx: mpsc::Sender<SessionNotification>,
    /// Reverse RPC bridge for fs/terminal tools.
    pub bridge: Arc<dyn ClientBridgeTrait>,
    /// Whether this connection is still alive. Set to `false` on disconnect.
    active: Arc<AtomicBool>,
}

impl std::fmt::Debug for AcpConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpConnection")
            .field("id", &self.id)
            .field("principal", &self.principal)
            .field("active", &self.is_active())
            .finish()
    }
}

impl AcpConnection {
    pub fn new(
        id: ConnectionId,
        principal: String,
        capabilities: ClientCapabilitiesInfo,
        notification_tx: mpsc::Sender<SessionNotification>,
        bridge: Arc<dyn ClientBridgeTrait>,
    ) -> Self {
        Self {
            id,
            principal,
            capabilities,
            notification_tx,
            bridge,
            active: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn deactivate(&self) {
        self.active.store(false, Ordering::Release);
    }
}
