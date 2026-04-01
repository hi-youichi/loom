//! Tests for telegram-bot configuration

#[cfg(test)]
mod tests {
    use telegram_bot::config::{load_config, BotConfig, Settings, TelegramBotConfig};
    use std::collections::HashMap;
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    fn setup_test_config() -> PathBuf {
        let temp_dir = env::temp_dir().join("telegram-bot-test");
        fs::create_dir_all(&temp_dir).ok();
        
        // Set LOOM_HOME to temp directory
        env::set_var("LOOM_HOME", &temp_dir);
        
        temp_dir
    }

    fn cleanup_test_config(temp_dir: &PathBuf) {
        env::remove_var("LOOM_HOME");
        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.download_dir, PathBuf::from("downloads"));
        assert_eq!(settings.log_level, "info");
    }

    #[test]
    fn test_bot_config_default() {
        let bot = BotConfig {
            token: "test_token".to_string(),
            enabled: true,
            description: None,
        };
        
        assert_eq!(bot.token, "test_token");
        assert!(bot.enabled);
    }

    #[test]
    fn test_config_validation() {
        let mut config = TelegramBotConfig::default();
        
        // Should fail: no bots configured
        assert!(config.validate().is_err());
        
        // Add a bot with empty token
        config.bots.insert(
            "test_bot".to_string(),
            BotConfig {
                token: "".to_string(),
                enabled: true,
                description: None,
            },
        );
        
        // Should fail: empty token
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_enabled_bots() {
        let mut config = TelegramBotConfig::default();
        
        config.bots.insert(
            "enabled_bot".to_string(),
            BotConfig {
                token: "token1".to_string(),
                enabled: true,
                description: None,
            },
        );
        
        config.bots.insert(
            "disabled_bot".to_string(),
            BotConfig {
                token: "token2".to_string(),
                enabled: false,
                description: None,
            },
        );
        
        let enabled = config.enabled_bots();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].0, "enabled_bot");
    }

    #[test]
    fn test_toml_parsing() {
        let toml_str = r#"
[settings]
download_dir = "custom_downloads"
log_level = "debug"

[bots.test_bot]
token = "test_token_123"
enabled = true
description = "Test bot"
"#;

        let config: TelegramBotConfig = toml::from_str(toml_str).unwrap();
        
        assert_eq!(config.settings.download_dir, PathBuf::from("custom_downloads"));
        assert_eq!(config.settings.log_level, "debug");
        assert_eq!(config.bots.len(), 1);
        
        let bot = &config.bots["test_bot"];
        assert_eq!(bot.token, "test_token_123");
        assert!(bot.enabled);
        assert_eq!(bot.description, Some("Test bot".to_string()));
    }

    #[test]
    fn test_env_var_interpolation() {
        // Set test environment variable
        env::set_var("TEST_BOT_TOKEN", "test_token_from_env");
        
        let toml_str = r#"
[bots.test_bot]
token = "${TEST_BOT_TOKEN}"
enabled = true
"#;

        // The interpolation should replace ${TEST_BOT_TOKEN} with the actual value
        let interpolated = telegram_bot::config::interpolate_env_vars(toml_str).unwrap();
        assert!(interpolated.contains("test_token_from_env"));
        
        env::remove_var("TEST_BOT_TOKEN");
    }
}
