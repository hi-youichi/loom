use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramBotConfig {
    #[serde(default)]
    pub settings: Settings,

    #[serde(default)]
    pub bots: HashMap<String, BotConfig>,

    #[serde(default)]
    pub agent: Option<AgentConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_download_dir")]
    pub download_dir: PathBuf,

    #[serde(default = "default_log_level")]
    pub log_level: String,

    #[serde(default)]
    pub log_file: Option<PathBuf>,

    #[serde(default = "default_polling_timeout")]
    pub polling_timeout: u64,

    #[serde(default = "default_retry_timeout")]
    pub retry_timeout: u64,

    #[serde(default)]
    pub only_respond_when_mentioned: bool,

    #[serde(default = "default_telegram_message_max_chars")]
    pub telegram_message_max_chars: usize,

    #[serde(default = "default_telegram_safe_reply_chars")]
    pub telegram_safe_reply_chars: usize,

    #[serde(default)]
    pub streaming: StreamingConfig,
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
            telegram_message_max_chars: default_telegram_message_max_chars(),
            telegram_safe_reply_chars: default_telegram_safe_reply_chars(),
            streaming: StreamingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingConfig {
    #[serde(default = "default_throttle_ms")]
    pub throttle_ms: u64,

    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    #[serde(default = "default_ack_placeholder_text")]
    pub ack_placeholder_text: String,

    #[serde(default = "default_busy_text")]
    pub busy_text: String,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            throttle_ms: default_throttle_ms(),
            max_retries: default_max_retries(),
            ack_placeholder_text: default_ack_placeholder_text(),
            busy_text: default_busy_text(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub token: String,

    #[serde(default = "default_enabled")]
    pub enabled: bool,

    pub description: Option<String>,

    pub handler: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub agent: String,

    pub system_prompt: Option<String>,

    #[serde(default)]
    pub memory: MemoryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_memory_limit")]
    pub max_messages: usize,
}

fn default_enabled() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_download_dir() -> PathBuf {
    PathBuf::from("downloads")
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_polling_timeout() -> u64 {
    30
}

fn default_retry_timeout() -> u64 {
    60
}

fn default_telegram_message_max_chars() -> usize {
    crate::constants::telegram::MESSAGE_MAX_CHARS
}

fn default_telegram_safe_reply_chars() -> usize {
    crate::constants::telegram::SAFE_REPLY_CHARS
}

fn default_throttle_ms() -> u64 {
    crate::constants::streaming::EDIT_THROTTLE_BASE_MS
}

fn default_max_retries() -> u32 {
    3
}

fn default_memory_limit() -> usize {
    100
}

fn default_ack_placeholder_text() -> String {
    "已收到，开始处理。处理时间较长时我会定期同步进展。".to_string()
}

fn default_busy_text() -> String {
    "上一个请求还在处理中，请稍后再发新消息。".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_values() {
        let s = Settings::default();
        assert_eq!(s.download_dir, PathBuf::from("downloads"));
        assert_eq!(s.log_level, "info");
        assert!(s.log_file.is_none());
        assert_eq!(s.polling_timeout, 30);
        assert_eq!(s.retry_timeout, 60);
        assert!(!s.only_respond_when_mentioned);
        assert_eq!(
            s.telegram_message_max_chars,
            crate::constants::telegram::MESSAGE_MAX_CHARS
        );
        assert_eq!(
            s.telegram_safe_reply_chars,
            crate::constants::telegram::SAFE_REPLY_CHARS
        );
    }

    #[test]
    fn streaming_config_default_values() {
        let s = StreamingConfig::default();
        assert_eq!(
            s.throttle_ms,
            crate::constants::streaming::EDIT_THROTTLE_BASE_MS
        );
        assert_eq!(s.max_retries, 3);
        assert!(!s.ack_placeholder_text.is_empty());
        assert!(!s.busy_text.is_empty());
    }

    #[test]
    fn telegram_bot_config_default() {
        let config = TelegramBotConfig::default();
        assert!(config.bots.is_empty());
        assert!(config.agent.is_none());
    }

    #[test]
    fn settings_serde_roundtrip() {
        let original = Settings::default();
        let json = serde_json::to_string(&original).unwrap();
        let parsed: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(original.download_dir, parsed.download_dir);
        assert_eq!(original.log_level, parsed.log_level);
        assert_eq!(original.polling_timeout, parsed.polling_timeout);
    }

    #[test]
    fn memory_config_default() {
        let mc = MemoryConfig::default();
        // Rust Default gives false/0, serde defaults are different
        assert!(!mc.enabled);
        assert_eq!(mc.max_messages, 0);
    }

    #[test]
    fn memory_config_serde_defaults() {
        // When deserialized from empty JSON object, serde defaults kick in
        let mc: MemoryConfig = serde_json::from_str("{}").unwrap();
        assert!(mc.enabled);
        assert_eq!(mc.max_messages, 100);
    }

    #[test]
    fn bot_config_deserialize() {
        let json = r#"{"token": "123456:ABC-DEF"}"#;
        let bot: BotConfig = serde_json::from_str(json).unwrap();
        assert_eq!(bot.token, "123456:ABC-DEF");
        assert!(bot.enabled);
        assert!(bot.description.is_none());
    }

    #[test]
    fn bot_config_disabled() {
        let json = r#"{"token": "t", "enabled": false}"#;
        let bot: BotConfig = serde_json::from_str(json).unwrap();
        assert!(!bot.enabled);
    }

    #[test]
    fn agent_config_deserialize() {
        let json = r#"{"agent": "dev"}"#;
        let agent: AgentConfig = serde_json::from_str(json).unwrap();
        assert_eq!(agent.agent, "dev");
        assert!(agent.system_prompt.is_none());
        // When memory field is missing, #[serde(default)] uses MemoryConfig::default()
        // which gives enabled=false (Rust Default, not serde default functions)
        assert!(!agent.memory.enabled);
        assert_eq!(agent.memory.max_messages, 0);
    }
}
