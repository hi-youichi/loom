//! Telegram Bot Multi-Bot Manager
//!
//! This module provides support for running multiple Telegram bots with long polling.
//!
//! # Configuration
//!
//! Uses loom's config system: `~/.loom/telegram-bot.toml`
//!
//! ```toml
//! [settings]
//! download_dir = "downloads"
//! log_level = "info"
//!
//! [bots.assistant]
//! token = "${TELOXIDE_TOKEN}"  # Environment variable interpolation
//! enabled = true
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! use telegram_bot::run_bots;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//!     run_bots().await?;
//!     Ok(())
//! }
//! ```
//! 

mod agent;
mod bot;
mod command;
mod config;
mod download;
mod error;
pub mod formatting;
mod handler;

mod handler_deps;
mod health;
mod metrics;
mod pipeline;
mod router;
mod sender;
mod session;
mod streaming;
mod traits;
pub mod utils;

/// Narrow re-exports for typical embedders and binaries.
pub mod prelude {
    pub use crate::config::{
        load_config, AgentConfig, BotConfig, ConfigError, InteractionMode, Settings,
        StreamingConfig, TelegramBotConfig,
    };
    pub use crate::error::{BotError, Result};
    pub use crate::{run_bots, run_with_config, BotManager};
}

/// Test doubles for integration tests and harnesses. Not used by the production binary.
pub mod mock;

pub use config::{
    load_config, load_from_path, TelegramBotConfig, BotConfig, Settings, AgentConfig,
    ConfigError, InteractionMode, StreamingConfig,
};
pub use bot::{run_bots, run_with_config, BotManager};
pub use error::{BotError, Result};
pub use health::{create_health_router, start_health_server, HealthState};
pub use metrics::{create_metrics_middleware, BotMetrics, MetricsSnapshot};
pub use download::{DownloadConfig, FileMetadata, FileType, TeloxideDownloader};
pub use handler::default_handler;
pub use handler_deps::{ChatRunRegistry, HandlerDeps};
pub use router::handle_message_with_deps;
pub use streaming::{run_loom_agent_streaming, stream_message_handler, StreamCommand};
pub use traits::{AgentRunContext, MessageSender, AgentRunner, SessionManager, FileDownloader};
pub use sender::TeloxideSender;
pub use agent::LoomAgentRunner;
pub use session::SqliteSessionManager;
