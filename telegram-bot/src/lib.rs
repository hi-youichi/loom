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
//! use telegram_bot::{run_bots, load_config};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Load config from ~/.loom/telegram-bot.toml
//!     let config = load_config()?;
//!     
//!     // Or from custom path
//!     // let config = load_config_from("path/to/config.toml")?;
//!     
//!     run_bots(config).await
//! }
//! ```

mod bot;
mod config;
mod handler;

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
