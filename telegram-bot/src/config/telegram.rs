//! Telegram Bot Configuration
//!
//! Loads from `~/.loom/telegram-bot.toml` with environment variable interpolation.
//! Integrates with loom's config system for future agent support.

use config::home::loom_home;
use std::path::{Path, PathBuf};

use crate::config::error::ConfigError;
use crate::config::types::{BotConfig, TelegramBotConfig};

impl TelegramBotConfig {
    pub fn get_enabled_bots(&self) -> Vec<(&String, &BotConfig)> {
        self.bots
            .iter()
            .filter(|(_, config)| config.enabled)
            .collect()
    }

    pub fn download_dir(&self) -> PathBuf {
        let dir = &self.settings.download_dir;
        if dir.is_absolute() {
            dir.clone()
        } else {
            loom_home().join(dir)
        }
    }
}

pub fn load_from_path(path: &Path) -> Result<TelegramBotConfig, ConfigError> {
    let content = std::fs::read_to_string(path)?;
    let interpolated = interpolate_env_vars(&content)?;
    let config: TelegramBotConfig = toml::from_str(&interpolated)?;

    if config.bots.is_empty() {
        return Err(ConfigError::NoBots);
    }

    for (name, bot_config) in &config.bots {
        if bot_config.token.is_empty() {
            return Err(ConfigError::MissingToken(name.clone()));
        }
    }

    Ok(config)
}



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
