//! Loom TUI Library Interface
//!
//! Public API for the enhanced TUI CLI implementation.

pub mod agent;
pub mod config;
pub mod session;
pub mod tui;
pub mod utils;

pub use agent::{AgentAdapter, AgentRuntime, AgentState};
pub use config::{Config, TuiConfig};
pub use session::{Session, SessionManager, SessionConfig};
pub use tui::{TuiEngine, TuiApp, TuiEvent};
pub use utils::{error::Result, terminal::TerminalCleanup};

/// Re-export commonly used types
pub use cli::{RunOptions, RunOutput, UserContent};
pub use loom_llm::message::Message;

/// Application entry point for library usage
pub async fn run_tui_session(config: Config) -> Result<()> {
    let mut app = TuiApp::new(config).await?;
    app.run().await
}