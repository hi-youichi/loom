//! WebSocket server for Loom (axum + ws).
//!
//! Listens on ws://127.0.0.1:8080, handles run, tools_list, tool_show, ping.
//!
//! **Public API**: [`run_serve`], [`run_serve_on_listener`].

mod app;
mod connection;
mod response;
mod run;
mod tools;
mod user_messages;

use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tracing::info;

use app::{router, run_config_from_env, AppState};

const DEFAULT_WS_ADDR: &str = "127.0.0.1:8080";

/// Runs the WebSocket server on an existing listener. Used by tests (bind to 127.0.0.1:0 then pass listener).
/// When `once` is true, accepts one connection, handles it, then returns.
pub async fn run_serve_on_listener(
    listener: TcpListener,
    once: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = listener.local_addr()?;
    info!("WebSocket server listening on ws://{}", addr);
    if once {
        info!("will exit after first connection is done (once mode, used by tests)");
    }

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let workspace_store = std::env::var("WORKSPACE_DB")
        .ok()
        .unwrap_or_else(|| "workspace.db".to_string());
    let workspace_store = loom_workspace::Store::new(&workspace_store).ok().map(Arc::new);
    let user_message_store = std::env::var("USER_MESSAGE_DB")
        .ok()
        .and_then(|path| loom::SqliteUserMessageStore::new(&path).ok())
        .map(|store| Arc::new(store) as Arc<dyn loom::UserMessageStore>);
    let state = Arc::new(AppState {
        shutdown_tx: Arc::new(std::sync::Mutex::new(if once {
            Some(shutdown_tx)
        } else {
            None
        })),
        workspace_store,
        user_message_store,
        run_config: run_config_from_env(),
    });

    let app = router(state);

    if once {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await?;
        info!("connection done, exiting (once mode)");
    } else {
        axum::serve(listener, app).await?;
    }
    Ok(())
}

/// Runs the WebSocket server. Listens on `addr` (default 127.0.0.1:8080).
/// When `once` is true, accepts one connection, handles it, then returns (process exits).
pub async fn run_serve(
    addr: Option<&str>,
    once: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = addr.unwrap_or(DEFAULT_WS_ADDR);
    let listener = TcpListener::bind(addr).await?;
    run_serve_on_listener(listener, once).await
}
