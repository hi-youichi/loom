//! Telegram Bot Configuration Module
//!
//! Integrates with loom's config system to load bot configuration from:
//! - `{loom_home}/telegram-bot.toml` (primary)
//! - Supports environment variable interpolation: `${TOKEN}`

mod error;
mod loader;
mod telegram;
mod types;

pub use error::ConfigError;
pub use loader::{load_config, load_from_path};
pub use types::{AgentConfig, BotConfig, Settings, StreamingConfig, TelegramBotConfig};
