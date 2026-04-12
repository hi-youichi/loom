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
use loom::llm::{ModelRegistry, ProviderConfig};
use config;

const DEFAULT_WS_ADDR: &str = "127.0.0.1:8080";

/// Runs the WebSocket server on an existing listener. Used by tests (bind to 127.0.0.1:0 then pass listener).
/// When `once` is true, accepts one connection, handles it, then returns.
pub async fn run_serve_on_listener(
    listener: TcpListener,
    once: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = listener.local_addr()?;
    info!("🌐 WebSocket server initializing on ws://{}", addr);
    if once {
        info!("🔧 Running in once mode (will exit after first connection is done, used by tests)");
    }

    // Setup basic components
    info!("📦 Setting up server components...");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let workspace_store = setup_workspace_store();
    let user_message_store = setup_user_message_store();
    info!("✅ Server components setup complete");
    
    // Initialize ModelRegistry and load providers from config
    info!("🔄 Initializing ModelRegistry...");
    let load_start = Instant::now();
    let registry = ModelRegistry::global();
    info!("✓ ModelRegistry initialized (took {}ms)", load_start.elapsed().as_millis());
    
    // Load provider configs from config file
    info!("📡 Loading providers from config file...");
    let providers: Vec<ProviderConfig> = match config::load_full_config("loom") {
        Ok(full_config) => {
            info!("✅ Loaded config with {} providers", full_config.providers.len());
            full_config.providers
                .into_iter()
                .map(|p| {
                    info!("  📋 Provider: {} ({})", p.name, p.provider_type);
                    ProviderConfig {
                        name: p.name,
                        base_url: p.base_url,
                        api_key: p.api_key,
                        provider_type: p.provider_type,
                        fetch_models: p.fetch_models.unwrap_or(false),
                    }
                })
                .collect()
        }
        Err(e) => {
            info!("⚠️  No config file found or error loading config: {}", e);
            vec![]
        }
    };
    
    // Load models from configured providers
    info!("🤖 Loading models from {} provider(s)...", providers.len());
    let load_start = Instant::now();
    let models = registry.list_all_models(&providers).await;
    let model_count = models.len();
    info!("✅ Model loading completed (took {}ms)", load_start.elapsed().as_millis());
    
    if model_count == 0 {
        error!("❌ No models loaded from configured providers");
        if providers.is_empty() {
            error!("💡 Hint: Create ~/.loom/config.toml with [[providers]] entries");
        }
        return Err("No models available".into());
    }
    
    info!("✅ Successfully loaded {} models from {} providers (total: {}ms)", 
        model_count, providers.len(), load_start.elapsed().as_millis());
    for (i, model) in models.iter().enumerate() {
        info!("  {}. {} ({})", i + 1, model.name, model.provider);
    }
    
    let run_config = RunConfig::default();
    
    info!("🚀 Starting server with configuration:");
    info!("  Event queue capacity: {}", run_config.event_queue_capacity);
    info!("  Append queue capacity: {}", run_config.append_queue_capacity);
    
    let state = Arc::new(AppState {
        shutdown_tx: Arc::new(std::sync::Mutex::new(if once {
            Some(shutdown_tx)
        } else {
            None
        })),
        workspace_store,
        user_message_store,
        run_config: run_config_from_env(),
        providers: Arc::new(providers),
    });

    let app = router(state);
    
    info!("✅ Server initialization complete, ready to accept connections");
    info!("📍 Listening on: {}", addr);
    info!("🔗 WebSocket endpoint: ws://{}/", addr);

    if once {
        info!("⏳ Waiting for first connection (once mode)...");
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await?;
        info!("✅ Connection completed, exiting (once mode)");
    } else {
        info!("🔄 Server running in persistent mode (will handle multiple connections)");
        axum::serve(listener, app).await?;
    }
    
    info!("🛑 Server shutdown complete");
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
