//! Axum app: state, router, and WebSocket upgrade handler.
//!
//! Single route: `GET /` upgrades to WebSocket; each connection is handled by [`handle_socket`]
//! with shared state (workspace store, user message store, run config, optional shutdown).

use axum::{
    extract::{ws::WebSocketUpgrade, State},
    response::Response,
    routing::get,
    Router,
};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

use super::connection::handle_socket;

/// Run-related server configuration (queue capacities and display limits).
#[derive(Clone)]
pub(crate) struct RunConfig {
    /// Max protocol events buffered between run task and WebSocket sender.
    pub(crate) event_queue_capacity: usize,
    /// Max (thread_id, message) pairs buffered for the append-to-store task.
    pub(crate) append_queue_capacity: usize,
    /// Max length for truncated display strings in run/tools.
    pub(crate) display_max_len: usize,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            event_queue_capacity: 128,
            append_queue_capacity: 64,
            display_max_len: 2000,
        }
    }
}

/// Builds RunConfig from environment variables, falling back to [`Default`] for unset or invalid values.
///
/// - `SERVE_EVENT_QUEUE_CAPACITY` (default 128)
/// - `SERVE_APPEND_QUEUE_CAPACITY` (default 64)
/// - `SERVE_DISPLAY_MAX_LEN` (default 2000)
pub(crate) fn run_config_from_env() -> RunConfig {
    let default = RunConfig::default();
    RunConfig {
        event_queue_capacity: std::env::var("SERVE_EVENT_QUEUE_CAPACITY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default.event_queue_capacity),
        append_queue_capacity: std::env::var("SERVE_APPEND_QUEUE_CAPACITY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default.append_queue_capacity),
        display_max_len: std::env::var("SERVE_DISPLAY_MAX_LEN")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default.display_max_len),
    }
}

/// Shared state for the WebSocket server.
///
/// Injected into the router and cloned per connection so handlers can access workspace
/// and user-message stores without passing them through every layer.
#[derive(Clone)]
pub(crate) struct AppState {
    /// When set, the first WebSocket connection to close will send on this to signal server exit (once mode).
    pub(crate) shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    /// When set, Run requests with workspace_id + thread_id register the thread in this workspace.
    pub(crate) workspace_store: Option<Arc<loom_workspace::Store>>,
    /// When set, user and assistant messages are appended per thread (Phase 2: stream-event driven).
    pub(crate) user_message_store: Option<std::sync::Arc<dyn loom::UserMessageStore>>,
    /// Run and tools configuration (queue capacities, display_max_len).
    pub(crate) run_config: RunConfig,
}

/// Builds the Axum router with a single WebSocket route at `/`.
pub(crate) fn router(state: Arc<AppState>) -> Router {
    Router::new().route("/", get(ws_handler)).with_state(state)
}

/// Handles `GET /`: upgrades to WebSocket and delegates to [`handle_socket`] with state clones.
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    let shutdown_tx = state.shutdown_tx.lock().ok().and_then(|mut g| g.take());
    let workspace_store = state.workspace_store.clone();
    let user_message_store = state.user_message_store.clone();
    let run_config = state.run_config.clone();
    ws.on_upgrade(move |socket| handle_socket(socket, shutdown_tx, workspace_store, user_message_store, run_config))
}
