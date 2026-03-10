//! Standalone HTTP server that receives GitHub webhooks (issues event), verifies signature, and logs payload.
//!
//! Usage: set GITHUB_WEBHOOK_SECRET and optionally GITHUB_TOKEN, then run:
//!   cargo run -p gh --bin gh-webhook
//! Listens on 0.0.0.0:8080 by default; override with GH_WEBHOOK_PORT.

use std::net::SocketAddr;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();
    let secret = std::env::var("GITHUB_WEBHOOK_SECRET").unwrap_or_else(|_| {
        eprintln!("warning: GITHUB_WEBHOOK_SECRET unset, webhook signature verification will fail");
        String::new()
    });
    let port: u16 = std::env::var("GH_WEBHOOK_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let app = gh::webhook_router(secret.into_bytes(), None);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!(%addr, "listening for GitHub webhooks on POST /webhook");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
