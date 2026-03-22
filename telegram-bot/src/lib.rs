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
//!     // Runs bots from ~/.loom/telegram-bot.toml
//!     run_bots().await
//! }
//! ```

mod bot;
mod config;
mod handler;
mod handler_new;

// Re-export config types
pub mod bot_config {
    pub use crate::config::{
        load_config, load_from_path, TelegramBotConfig, BotConfig, Settings, AgentConfig,
        ConfigError,
    };
}

pub use bot::{run_bots, run_with_config, BotManager};
pub use config::{
    load_config, load_from_path, TelegramBotConfig, BotConfig, Settings, AgentConfig,
    ConfigError,
};
pub use handler::{default_handler, DownloadConfig};
