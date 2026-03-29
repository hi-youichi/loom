//! Configuration loader with environment variable interpolation
//!
//! Loads configuration from `~/.loom/telegram-bot.toml` or `LOOM_HOME/telegram-bot.toml`.

use crate::config::error::ConfigError;
use crate::config::types::TelegramBotConfig;
use config::home::loom_home;
use std::path::{Path, PathBuf};
use tracing::info;

const CONFIG_FILE: &str = "telegram-bot.toml";

pub fn load_config() -> Result<TelegramBotConfig, ConfigError> {
    let candidates = [
        loom_home().join(CONFIG_FILE),
        PathBuf::from(CONFIG_FILE),
    ];

    for path in &candidates {
        if path.exists() {
            info!("Loading config from: {}", path.display());
            return load_from_path(path);
        }
    }

    Err(ConfigError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!(
            "No config file found. Searched: {}",
            candidates
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    )))
}

pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<TelegramBotConfig, ConfigError> {
    crate::config::telegram::load_from_path(path.as_ref())
}
