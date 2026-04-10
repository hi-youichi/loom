//! WebSocket server for Loom (axum + ws).
//!
//! Listens on ws://127.0.0.1:8080, handles run, tools_list, tool_show, agent_list, workspace_*, ping.
//!
//! **Public API**: [`run_serve`], [`run_serve_on_listener`].

mod app;
mod workspace;
mod connection;
mod response;
mod run;
mod tools;
mod user_messages;
mod agents;
mod models;

use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tracing::{error, info};

use app::{router, run_config_from_env, AppState};
use loom::services::ModelService;

const DEFAULT_WS_ADDR: &str = "127.0.0.1:8080";

/// Runs the WebSocket server on an existing listener. Used by tests (bind to 127.0.0.1:0 then pass listener).
/// When `once` is true, accepts one connection, handles it, then returns.
pub async fn run_serve_on_listener(
    listener: TcpListener,
    once: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = listener.local_addr()?;
    info!("WebSocket server initializing on ws://{}", addr);
    if once {
        info!("will exit after first connection is done (once mode, used by tests)");
    }

    // Setup basic components
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let workspace_store = setup_workspace_store();
    let user_message_store = setup_user_message_store();
    
    // Initialize ModelService
    info!("🔄 Initializing ModelService...");
    let load_start = Instant::now();
    let model_service = Arc::new(ModelService::new());
    info!("✓ ModelService initialized (took {}ms)", load_start.elapsed().as_millis());
    
    // Load models from models.dev (BLOCKING - required for server to start)
    info!("📡 Loading models from models.dev API...");
    let api_load_start = Instant::now();
    
    match model_service.load_from_models_dev().await {
        Ok(_) => {
            let duration = api_load_start.elapsed();
            let models = model_service.get_available_models().await;
            
            if models.is_empty() {
                error!("❌ No models loaded from models.dev, cannot start server");
                return Err("No models available from models.dev API".into());
            }
            
            info!("✅ Successfully loaded {} models from models.dev (took {}ms)", 
                models.len(), duration.as_millis());
        }
        Err(e) => {
            let duration = api_load_start.elapsed();
            error!("❌ Failed to load models from models.dev after {}ms: {}", 
                duration.as_millis(), e);
            return Err(format!("Failed to load models from models.dev: {}", e).into());
        }
    }
    
    // Final verification
    let final_models = model_service.get_available_models().await;
    if final_models.is_empty() {
        error!("❌ Final verification failed - no models available");
        return Err("No models available after initialization".into());
    }
    
    let total_duration = load_start.elapsed();
    info!("🚀 Model initialization complete: {} models available (total: {}ms)", 
        final_models.len(), total_duration.as_millis());
    
    // Log sample models
    for (i, model) in final_models.iter().take(5).enumerate() {
        info!("  {}. {} ({})", i + 1, model.name, model.provider);
    }

    let state = Arc::new(AppState {
        shutdown_tx: Arc::new(std::sync::Mutex::new(if once {
            Some(shutdown_tx)
        } else {
            None
        })),
        workspace_store,
        user_message_store,
        run_config: run_config_from_env(),
        model_service,
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

/// Setup workspace store from environment
fn setup_workspace_store() -> Option<Arc<loom_workspace::Store>> {
    let db_path = std::env::var("WORKSPACE_DB")
        .ok()
        .unwrap_or_else(|| "workspace.db".to_string());
    
    loom_workspace::Store::new(&db_path)
        .ok()
        .map(Arc::new)
        .inspect(|_| info!("✓ Workspace store initialized"))
}

/// Setup user message store from environment
fn setup_user_message_store() -> Option<std::sync::Arc<dyn loom::UserMessageStore>> {
    let db_path = std::env::var("USER_MESSAGE_DB")
        .ok()
        .and_then(|path| loom::SqliteUserMessageStore::new(&path).ok())
        .map(|store| Arc::new(store) as Arc<dyn loom::UserMessageStore>);
    
    if db_path.is_some() {
        info!("✓ User message store initialized");
    } else {
        info!("ℹ️  User message store not configured (optional feature)");
    }
    
    db_path
}
