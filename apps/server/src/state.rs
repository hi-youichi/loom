//! In-memory state shared across all routes — ACP-only server.

use std::sync::Arc;

/// Shared handle handed to every route via `axum::extract::State`.
pub type SharedState = Arc<AppState>;

/// All mutable state lives behind locks. The ACP runtime manages its own
/// session/persistence via `anureo_acp`; this struct only holds the hub.
pub struct AppState {
    /// Durable ACP sessions and notification routing for `/acp` reconnects.
    pub acp_hub: Arc<crate::acp_hub::AcpHub>,
}

/// Build an isolated state for tests and in-process callers.
pub fn new_state() -> SharedState {
    Arc::new(AppState {
        acp_hub: Arc::new(crate::acp_hub::AcpHub::default()),
    })
}

/// Build production server state.
pub fn new_server_state() -> SharedState {
    new_state()
}

/// Build production server state with a specific ACP hub (used by the
/// test server binary).
#[cfg(feature = "test-support")]
pub fn new_server_state_with_acp_hub(acp_hub: Arc<crate::acp_hub::AcpHub>) -> SharedState {
    Arc::new(AppState { acp_hub })
}
