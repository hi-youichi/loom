//! Configuration loader with environment variable interpolation
//!
//! Loads configuration from `~/.loom/telegram-bot.toml` or `LOOM_HOME/telegram-bot.toml`.

use crate::config::telegram::{BotConfig, ConfigError, TelegramBotConfig};
use std::path::{Path, PathBuf};
use tracing::info;

/// Configuration file name
const CONFIG_FILE: &str = "telegram-bot.toml";

/// Load telegram-bot configuration
///
/// Search order:
/// 1. `LOOM_HOME/telegram-bot.toml` (or `~/.loom/telegram-bot.toml`)
/// 2. `./telegram-bot.toml` (current directory)
pub fn load_config() -> Result<TelegramBotConfig, ConfigError> {
    // Try LOOM_HOME first, then current directory
    let config_path = if let Some(path) = from_loom_home() {
        info!("Found config at: {:?}", path);
        path
    } else if let Some(path) = from_current_dir() {
        info!("Found config at: {:?}", path);
        path
    } else {
        return Err(ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No telegram-bot.toml found in LOOM_HOME or current directory"
        )));
    };
    
    TelegramBotConfig::from_file(&config_path)
}

/// Load configuration from a specific path
pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<TelegramBotConfig, ConfigError> {
    TelegramBotConfig::from_file(path)
}

/// Try to find config in LOOM_HOME directory
fn from_loom_home() -> Option<PathBuf> {
    // Use config crate's loom_home function
    let home = get_loom_home()?;
    let path = home.join(CONFIG_FILE);
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

/// Get LOOM_HOME directory
fn get_loom_home() -> Option<PathBuf> {
    // Check LOOM_HOME env var first
    if let Ok(home) = std::env::var("LOOM_HOME") {
        return Some(PathBuf::from(home));
    }
    
    // Otherwise use HOME/.loom or USERPROFILE/.loom
    #[cfg(unix)]
    {
        std::env::var("HOME")
            .map(|h| PathBuf::from(h).join(".loom"))
            .ok()
    }
    
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE")
            .map(|h| PathBuf::from(h).join(".loom"))
            .ok()
    }
}

/// Try to find config in current directory
fn from_current_dir() -> Option<PathBuf> {
    let path = std::env::current_dir().ok()?.join(CONFIG_FILE);
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loom_home_env() {
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", "/tmp/test-loom");
        let home = get_loom_home();
        assert_eq!(home, Some(PathBuf::from("/tmp/test-loom")));
        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }
}
