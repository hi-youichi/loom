//! Standalone HTTP server that receives GitHub webhooks (issues event), verifies signature, and logs payload.
//!
//! Options: CLI args override env. Example:
//!   gh-webhook --port 9000
//!   gh-webhook --secret "$GITHUB_WEBHOOK_SECRET" --bind 127.0.0.1 --port 8080
//!   gh-webhook --log-level debug --log-file /var/log/gh-webhook.log

use std::net::SocketAddr;

use clap::Parser;
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(name = "gh-webhook")]
#[command(about = "GitHub webhook server for issues events (spawns Loom agent)")]
struct Args {
    /// Webhook secret for X-Hub-Signature-256. Falls back to GITHUB_WEBHOOK_SECRET.
    #[arg(long, short = 's', env = "GITHUB_WEBHOOK_SECRET")]
    secret: Option<String>,

    /// Port to listen on. Falls back to GH_WEBHOOK_PORT, then 8080.
    #[arg(long, short = 'p', env = "GH_WEBHOOK_PORT", default_value_t = 8080)]
    port: u16,

    /// Bind address (e.g. 0.0.0.0 or 127.0.0.1). Falls back to GH_WEBHOOK_BIND, then 0.0.0.0.
    #[arg(long, short = 'b', env = "GH_WEBHOOK_BIND", default_value = "0.0.0.0")]
    bind: String,

    /// Log level (error, warn, info, debug, trace). Env: GH_WEBHOOK_LOG_LEVEL. Overrides RUST_LOG when set.
    #[arg(long, env = "GH_WEBHOOK_LOG_LEVEL", default_value = "info")]
    log_level: String,

    /// Log file path. When set, logs are also written to this file (in addition to stderr). Env: GH_WEBHOOK_LOG_FILE.
    #[arg(long, env = "GH_WEBHOOK_LOG_FILE")]
    log_file: Option<std::path::PathBuf>,
}

fn init_tracing(
    log_level: &str,
    log_file: Option<&std::path::Path>,
) -> Result<Option<tracing_appender::non_blocking::WorkerGuard>, Box<dyn std::error::Error + Send + Sync>>
{
    let filter = EnvFilter::try_new(log_level)
        .or_else(|_| EnvFilter::try_new("info"))
        .expect("valid log level");
    let fmt_layer = fmt::layer().with_target(true);

    let guard = if let Some(path) = log_file {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        let (writer, guard) = tracing_appender::non_blocking(file);
        let file_layer = fmt::layer()
            .with_ansi(false)
            .with_target(true)
            .with_writer(writer);
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .with(file_layer)
            .init();
        Some(guard)
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .init();
        None
    };
    Ok(guard)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();
    let _log_guard = init_tracing(&args.log_level, args.log_file.as_deref())?;

    let secret = args.secret.unwrap_or_else(|| {
        eprintln!("warning: GITHUB_WEBHOOK_SECRET unset, webhook signature verification will fail");
        String::new()
    });

    let addr: SocketAddr = format!("{}:{}", args.bind, args.port)
        .parse()
        .expect("valid bind:port");

    let app = gh::webhook_router(secret.into_bytes(), None);
    info!(%addr, "listening for GitHub webhooks on POST /webhook");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
