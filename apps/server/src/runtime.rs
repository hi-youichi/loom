use std::net::SocketAddr;

use clap::Args as ClapArgs;
use tokio::net::TcpListener;
use tracing_subscriber::{fmt, EnvFilter};

use crate::logging::{init_logging, LogConfig};
use crate::{routes::build_router, state::new_server_state};

#[derive(ClapArgs, Debug, Clone)]
pub struct ServerOptions {
    /// Bind port (0 = let the OS pick).
    #[arg(long, default_value_t = 3030)]
    pub port: u16,

    /// Bind host.
    #[arg(long = "host", visible_alias = "hostname", default_value = "127.0.0.1")]
    pub host: String,

    /// Optional directory to expose as the active working directory.
    #[arg(long)]
    pub directory: Option<String>,

    /// Enable verbose tracing (RUST_LOG-like).
    #[arg(long)]
    pub verbose: bool,
}

/// Run the HTTP + ACP-WebSocket server.
///
/// `log_config` — when `Some`, initializes file-based logging (level / file /
/// rotate / format from the global CLI flags, merged with `config.toml
/// [logging]`). When `None`, falls back to the legacy `--verbose` console
/// behavior.
pub async fn run(
    options: ServerOptions,
    log_config: Option<LogConfig>,
) -> Result<(), Box<dyn std::error::Error>> {
    let _log_guard = match log_config {
        Some(config) => Some(init_logging(&config)),
        // Fallback: legacy verbose-only console logging.
        None => {
            let filter = if options.verbose {
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
            } else {
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"))
            };
            let _ = fmt()
                .with_env_filter(filter)
                .with_target(false)
                .try_init();
            None
        }
    };

    if let Some(directory) = &options.directory {
        std::env::set_current_dir(directory)?;
    }

    let app = build_router(new_server_state());
    let address: SocketAddr = format!("{}:{}", options.host, options.port).parse()?;
    let listener = TcpListener::bind(address).await?;
    let bound = listener.local_addr()?;
    println!("loom server listening on http://{bound}");

    #[cfg(unix)]
    tokio::spawn(async {
        use tokio::signal::unix::{signal, SignalKind};

        let Ok(mut sighup) = signal(SignalKind::hangup()) else {
            tracing::warn!("failed to install SIGHUP handler");
            return;
        };
        while sighup.recv().await.is_some() {
            tracing::info!("SIGHUP received — reloading config.toml");
            match config::load_full_config("loom") {
                Ok(cfg) => tracing::info!(
                    providers = cfg.providers.len(),
                    default = cfg.default_provider.as_deref().unwrap_or("(none)"),
                    "SIGHUP reload OK"
                ),
                Err(error) => tracing::error!(%error, "SIGHUP reload FAILED — keeping previous config"),
            }
        }
    });

    axum::serve(listener, app).await?;
    Ok(())
}
