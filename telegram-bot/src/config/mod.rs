//! Telegram Bot Configuration Module
//!
//! Integrates with loom's config system to load bot configuration from:
//! - `$LOOM_HOME/telegram-bot.toml` (primary)
//! - Supports environment variable interpolation: `${TOKEN}`

mod loader;
mod telegram;

pub use loader::{load_config, load_from_path};
pub use telegram::{
    AgentConfig, BotConfig, ConfigError, Settings, StreamingConfig, TelegramBotConfig,
};
