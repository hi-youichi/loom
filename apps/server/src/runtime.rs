use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Args as ClapArgs;
use tokio::net::TcpListener;
use tracing_subscriber::{fmt, EnvFilter};

use crate::logging::{init_logging, LogConfig};
use crate::routes::build_router_with_static;
use crate::{pid_file, state::new_server_state};

#[derive(ClapArgs, Debug, Clone)]
pub struct ServerOptions {
    /// Bind port (0 = let the OS pick).
    #[arg(long, default_value_t = 3030)]
    pub port: u16,

    /// Bind host.
    #[arg(long = "host", visible_alias = "hostname", default_value = "127.0.0.1")]
    pub host: String,

    /// Enable verbose tracing (RUST_LOG-like).
    #[arg(long)]
    pub verbose: bool,

    /// PID file used to prevent concurrent server instances.
    #[arg(long, value_name = "PATH")]
    pub pid_file: Option<PathBuf>,

    /// Serve a built frontend (e.g. Loom Desk `packages/web/dist`) from this
    /// directory: static assets + SPA fallback on the same origin as the API.
    #[arg(long, value_name = "DIR")]
    pub static_dir: Option<PathBuf>,
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
            let _ = fmt().with_env_filter(filter).with_target(false).try_init();
            None
        }
    };

    let pid_path = pid_file::resolve_path(options.pid_file.as_deref());
    let _pid_guard = pid_file::PidFileGuard::acquire(&pid_path).map_err(|error| {
        format!(
            "cannot acquire server PID lock '{}': another Loom server may already be running ({error})",
            pid_path.display()
        )
    })?;

    let app = build_router_with_static(new_server_state(), options.static_dir.clone());
    let address: SocketAddr = format!("{}:{}", options.host, options.port).parse()?;
    let listener = TcpListener::bind(address).await?;
    let bound = listener.local_addr()?;
    println!("loom server listening on http://{bound}");

    if let Some(dir) = &options.static_dir {
        if dir.is_dir() {
            tracing::info!(dir = %dir.display(), "serving static frontend");
        } else {
            tracing::warn!(dir = %dir.display(), "--static-dir not found; static routes will 404");
        }
    }

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
                Err(error) => {
                    tracing::error!(%error, "SIGHUP reload FAILED — keeping previous config")
                }
            }
        }
    });

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let ctrl_c = tokio::signal::ctrl_c();
        let terminate = async {
            match signal(SignalKind::terminate()) {
                Ok(mut stream) => {
                    stream.recv().await;
                }
                Err(_) => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            _ = ctrl_c => {}
            _ = terminate => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }

    tracing::info!("shutdown signal received");
}
