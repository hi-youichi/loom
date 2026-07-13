//! `loom-server` — Loom agent core exposed as an HTTP+SSE server
//! compatible with the opencode TUI External mode.

use std::net::SocketAddr;

use clap::{Args as ClapArgs, Parser, Subcommand};
use loom_server::{routes::build_router, state::new_server_state};
use tokio::net::TcpListener;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser, Debug)]
#[command(
    name = "loom-server",
    version,
    about = "Loom agent kernel exposed as HTTP+SSE for the opencode TUI"
)]
struct Cli {
    /// Explicit server subcommand used by the rollout documentation.
    #[command(subcommand)]
    command: Option<Command>,

    /// Backwards-compatible direct flags (`loom-server --port 0`).
    #[command(flatten)]
    direct: ServeArgs,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Start the HTTP+SSE protocol server.
    Serve(ServeArgs),
}

#[derive(ClapArgs, Debug, Clone)]
struct ServeArgs {
    /// Bind port (0 = let the OS pick).
    #[arg(long, default_value_t = 0)]
    port: u16,

    /// Bind host.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Optional directory to expose as the active working directory.
    #[arg(long)]
    directory: Option<String>,

    /// Enable verbose tracing (RUST_LOG-like).
    #[arg(long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let args = match cli.command {
        Some(Command::Serve(args)) => args,
        None => cli.direct,
    };

    let filter = if args.verbose {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"))
    };
    fmt().with_env_filter(filter).with_target(false).init();

    if let Some(directory) = &args.directory {
        std::env::set_current_dir(directory)?;
    }

    let app = build_router(new_server_state());
    let address: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    let listener = TcpListener::bind(address).await?;
    let bound = listener.local_addr()?;
    println!("loom-server listening on http://{bound}");

    axum::serve(listener, app).await?;
    Ok(())
}
