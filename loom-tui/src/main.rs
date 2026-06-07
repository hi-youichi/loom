//! Loom TUI - Enhanced CLI with bottom input bar and status display
//!
//! A new CLI implementation that builds upon the existing Loom agent system
//! but adds a rich TUI interface with persistent input and status display.

mod agent;
mod config;
mod session;
mod tui;
mod utils;

pub use agent::{AgentAdapter, AgentRuntime};
pub use config::{Config, TuiConfig};
pub use session::{Session, SessionManager};
pub use tui::{TuiEngine, TuiEvent};

use clap::Parser;
use tracing::{error, info};
use utils::{error::Result, terminal::setup_terminal};

use crate::agent::AgentAdapter;
use crate::config::Args;
use crate::tui::LoomTui;

/// Application entry point for the enhanced TUI CLI.
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    init_logging()?;
    
    // Parse command line arguments
    let args = Args::parse();
    
    // Initialize terminal
    let cleanup = setup_terminal()?;
    
    // Load configuration
    let config = Config::load(&args)?;
    
    // Create and run TUI application
    let mut app = LoomTui::new(config).await?;
    let result = app.run().await;
    
    // Cleanup terminal
    let _ = cleanup;
    
    // Log result
    match result {
        Ok(_) => info!("Loom TUI completed successfully"),
        Err(e) => error!("Loom TUI failed: {}", e),
    }
    
    Ok(())
}

fn init_logging() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .init();
    
    Ok(())
}