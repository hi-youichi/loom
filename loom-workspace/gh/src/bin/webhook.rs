//! Standalone HTTP server that receives GitHub webhooks (issues event), verifies signature, and logs payload.
//!
//! Options: CLI args override env. Example:
//!   gh-webhook --port 9000
//!   gh-webhook --secret "$GITHUB_WEBHOOK_SECRET" --bind 127.0.0.1 --port 8080

use std::net::SocketAddr;

use clap::Parser;
use tracing::info;

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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

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
