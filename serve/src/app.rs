//! Axum app: state, router, and WebSocket upgrade handler.

use axum::{
    extract::{ws::WebSocketUpgrade, State},
    response::Response,
    routing::get,
    Router,
};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

use super::connection::handle_socket;

/// When set, the first WebSocket connection to close will send on this to signal server exit (once mode).
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

pub(crate) fn router(state: Arc<AppState>) -> Router {
    Router::new().route("/", get(ws_handler)).with_state(state)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    let shutdown_tx = state.shutdown_tx.lock().ok().and_then(|mut g| g.take());
    ws.on_upgrade(move |socket| handle_socket(socket, shutdown_tx))
}
