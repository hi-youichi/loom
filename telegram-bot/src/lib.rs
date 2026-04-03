//! Telegram Bot Multi-Bot Manager
//!
//! Production-grade Telegram bot framework built on [teloxide] with:
//!
//! - **Multi-bot support** — run several bots from a single process via long polling
//! - **Streaming agent** — real-time Think / Act streaming powered by Loom
//! - **Slash commands** — extensible [`CommandDispatcher`] for `/model`, `/reset`, `/help`
//! - **Media downloads** — photos, videos, documents with metadata
//! - **Model selection** — SQLite-backed model catalog with fuzzy search
//! - **Health & metrics** — axum health endpoint + atomic counters
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
//! token = "${TELOXIDE_TOKEN}"
//! enabled = true
//! ```
//!
//! # Quick start
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
//! [teloxide]: https://github.com/teloxide/teloxide

mod agent;
mod bot;
mod command;
mod config;
pub(crate) mod constants;
mod download;
mod error;
pub mod formatting;
mod handler_deps;
mod health;
mod metrics;
mod model_selection;
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

pub use agent::LoomAgentRunner;
pub use bot::{run_bots, run_with_config, BotManager};
pub use config::{
    load_config, load_from_path, AgentConfig, BotConfig, ConfigError, InteractionMode, Settings,
    StreamingConfig, TelegramBotConfig,
};
pub use download::{DownloadConfig, FileMetadata, FileType, TeloxideDownloader};
pub use error::{BotError, Result};
pub use handler_deps::{ChatRunRegistry, HandlerDeps};
pub use health::{create_health_router, start_health_server, HealthState};
pub use metrics::{BotMetrics, MetricsSnapshot};
pub use model_selection::{
    InMemorySearchSessionStore, ModelChoice, ModelSelectionService, SqliteModelSelectionStore,
    StaticModelCatalog,
};
pub use router::default_handler;
pub use router::handle_message_with_deps;
pub use sender::TeloxideSender;
pub use session::SqliteSessionManager;
pub use streaming::{
    run_loom_agent_streaming, stream_message_handler, stream_message_handler_simple, StreamCommand,
};
pub use traits::{AgentRunContext, AgentRunner, FileDownloader, MessageSender, SessionManager};
