//! Telegram Bot Configuration
//!
//! Loads from `~/.loom/telegram-bot.toml` with environment variable interpolation.
//! Integrates with loom's config system for future agent support.

use config::home::loom_home;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::info;

/// Configuration errors
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Environment variable not found: {0}")]
    EnvVarNotFound(String),

    #[error("No bots configured")]
    NoBots,

    #[error("Bot '{0}' has no token configured")]
    MissingToken(String),
}

/// Root telegram-bot configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramBotConfig {
    /// Global settings
    #[serde(default)]
    pub settings: Settings,

    /// Bot configurations
    #[serde(default)]
    pub bots: HashMap<String, BotConfig>,

    /// Agent integration (for future use)
    #[serde(default)]
    pub agent: Option<AgentConfig>,
}

impl TelegramBotConfig {
    /// Configuration file name
    const CONFIG_FILE: &'static str = "telegram-bot.toml";

    /// Load configuration from loom home directory
    ///
    /// Looks for `~/.loom/telegram-bot.toml`
    pub fn load() -> Result<Self, ConfigError> {
        let path = loom_home().join(Self::CONFIG_FILE);
        info!("Loading telegram-bot config from: {:?}", path);
        Self::from_file(path)
    }

    /// Load from a specific file path
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(&path)?;

        // Apply environment variable interpolation
        let interpolated = interpolate_env_vars(&content)?;

        let config: TelegramBotConfig = toml::from_str(&interpolated)?;
        config.validate()?;

        info!("Loaded {} bots from config", config.bots.len());
        Ok(config)
    }

    /// Validate configuration
    fn validate(&self) -> Result<(), ConfigError> {
        if self.bots.is_empty() {
            return Err(ConfigError::NoBots);
        }

        for (name, bot) in &self.bots {
            if bot.token.is_empty() {
                return Err(ConfigError::MissingToken(name.clone()));
            }
        }

        Ok(())
    }

    /// Get enabled bots
    pub fn enabled_bots(&self) -> Vec<(&String, &BotConfig)> {
        self.bots.iter().filter(|(_, bot)| bot.enabled).collect()
    }

    /// Get download directory (resolved to absolute path)
    pub fn download_dir(&self) -> PathBuf {
        let dir = &self.settings.download_dir;
        if dir.is_absolute() {
            dir.clone()
        } else {
            loom_home().join(dir)
        }
    }
}

impl Default for TelegramBotConfig {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            bots: HashMap::new(),
            agent: None,
        }
    }
}

/// Global settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Download directory for media files
    #[serde(default = "default_download_dir")]
    pub download_dir: PathBuf,

    /// Log level
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Log file path (optional, relative to loom home or absolute)
    #[serde(default)]
    pub log_file: Option<PathBuf>,

    /// Polling timeout in seconds
    #[serde(default = "default_polling_timeout")]
    pub polling_timeout: u64,

    /// Retry timeout on network errors
    #[serde(default = "default_retry_timeout")]
    pub retry_timeout: u64,

    /// Only respond when bot is mentioned (@username)
    #[serde(default)]
    pub only_respond_when_mentioned: bool,

    /// Streaming display configuration
    #[serde(default)]
    pub streaming: StreamingConfig,
}

/// Streaming display configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingConfig {
    /// Maximum characters to display in Think phase (0 = unlimited)
    #[serde(default = "default_max_think_chars")]
    pub max_think_chars: usize,

    /// Maximum characters to display in Act phase (0 = unlimited)
    #[serde(default = "default_max_act_chars")]
    pub max_act_chars: usize,

    /// Whether to show Think phase
    #[serde(default = "default_show_think_phase")]
    pub show_think_phase: bool,

    /// Whether to show Act phase
    #[serde(default = "default_show_act_phase")]
    pub show_act_phase: bool,

    /// Emoji for Think messages
    #[serde(default = "default_think_emoji")]
    pub think_emoji: String,

    /// Emoji for Act messages
    #[serde(default = "default_act_emoji")]
    pub act_emoji: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: default_download_dir(),
            log_level: default_log_level(),
            log_file: None,
            polling_timeout: default_polling_timeout(),
            retry_timeout: default_retry_timeout(),
            only_respond_when_mentioned: false,
            streaming: StreamingConfig::default(),
        }
    }
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            max_think_chars: default_max_think_chars(),
            max_act_chars: default_max_act_chars(),
            show_think_phase: default_show_think_phase(),
            show_act_phase: default_show_act_phase(),
            think_emoji: default_think_emoji(),
            act_emoji: default_act_emoji(),
        }
    }
}

fn default_max_think_chars() -> usize {
    500
}

fn default_max_act_chars() -> usize {
    500
}

fn default_show_think_phase() -> bool {
    true
}

fn default_show_act_phase() -> bool {
    true
}

fn default_think_emoji() -> String {
    "🤔".to_string()
}

fn default_act_emoji() -> String {
    "⚡".to_string()
}

fn default_download_dir() -> PathBuf {
    PathBuf::from("telegram-bot-downloads")
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_polling_timeout() -> u64 {
    30
}

fn default_retry_timeout() -> u64 {
    5
}

/// Individual bot configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    /// Bot token (supports ${ENV_VAR} interpolation)
    pub token: String,

    /// Whether this bot is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Bot description
    pub description: Option<String>,

    /// Custom handler module path (for future use)
    pub handler: Option<String>,
}

fn default_enabled() -> bool {
    true
}

/// Agent integration configuration (for future use)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Agent name to use for this bot
    pub agent: String,

    /// System prompt template
    pub system_prompt: Option<String>,

    /// Memory configuration
    #[serde(default)]
    pub memory: MemoryConfig,
}

/// Memory configuration for agent
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryConfig {
    /// Enable conversation memory
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum messages to remember
    #[serde(default = "default_memory_limit")]
    pub max_messages: usize,
}

fn default_true() -> bool {
    true
}

fn default_memory_limit() -> usize {
    100
}

/// Interpolate environment variables in config content
///
/// Supports: ${VAR_NAME} or $VAR_NAME
fn interpolate_env_vars(content: &str) -> Result<String, ConfigError> {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::with_capacity(lines.len());

    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            result.push(line.to_string());
            continue;
        }

        let mut processed = line.to_string();
        let mut start = 0;

        while let Some(dollar_pos) = processed[start..].find('$') {
            let dollar_pos = start + dollar_pos;

            if dollar_pos > 0 && processed.chars().nth(dollar_pos - 1) == Some('\\') {
                start = dollar_pos + 1;
                continue;
            }

            let (var_name, end_pos) = if processed[dollar_pos + 1..].starts_with('{') {
                let brace_end = processed[dollar_pos + 2..].find('}').ok_or_else(|| {
                    ConfigError::EnvVarNotFound(format!(
                        "Unclosed brace at position {}",
                        dollar_pos
                    ))
                })?;
                let var_name = processed[dollar_pos + 2..dollar_pos + 2 + brace_end].to_string();
                (var_name, dollar_pos + 2 + brace_end + 1)
            } else {
                let end = processed[dollar_pos + 1..]
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .map(|i| dollar_pos + 1 + i)
                    .unwrap_or(processed.len());
                let var_name = processed[dollar_pos + 1..end].to_string();
                (var_name, end)
            };

            let value = std::env::var(&var_name)
                .map_err(|_| ConfigError::EnvVarNotFound(var_name.clone()))?;

            processed.replace_range(dollar_pos..end_pos, &value);
            start = dollar_pos + value.len();
        }

        result.push(processed);
    }

    Ok(result.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpolate_env_vars() {
        std::env::set_var("TEST_TOKEN", "123:ABC");
        std::env::set_var("TEST_DIR", "/tmp/downloads");

        let input = r#"
            token = "${TEST_TOKEN}"
            dir = "$TEST_DIR"
        "#;

        let result = interpolate_env_vars(input).unwrap();
        assert!(result.contains("123:ABC"));
        assert!(result.contains("/tmp/downloads"));
    }
}
