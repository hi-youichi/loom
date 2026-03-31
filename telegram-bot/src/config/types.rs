use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum InteractionMode {
    Streaming,
    #[default]
    PeriodicSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramBotConfig {
    #[serde(default)]
    pub settings: Settings,

    #[serde(default)]
    pub bots: HashMap<String, BotConfig>,

    pub agent: Option<AgentConfig>,
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
    #[serde(default)]
    pub interaction_mode: InteractionMode,

    #[serde(default = "default_max_act_chars")]
    pub max_act_chars: usize,

    #[serde(default = "default_show_act_phase")]
    pub show_act_phase: bool,

    #[serde(default = "default_act_emoji")]
    pub act_emoji: String,

    #[serde(default = "default_throttle_ms")]
    pub throttle_ms: u64,

    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    #[serde(default = "default_summary_interval_secs")]
    pub summary_interval_secs: u64,

    #[serde(default = "default_periodic_summary_ms")]
    pub periodic_summary_ms: u64,

    #[serde(default = "default_max_tool_result_chars")]
    pub max_tool_result_chars: usize,

    #[serde(default = "default_ack_placeholder_text")]
    pub ack_placeholder_text: String,

    #[serde(default = "default_busy_text")]
    pub busy_text: String,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            interaction_mode: InteractionMode::default(),
            max_act_chars: default_max_act_chars(),
            show_act_phase: default_show_act_phase(),
            act_emoji: default_act_emoji(),
            throttle_ms: default_throttle_ms(),
            max_retries: default_max_retries(),
            summary_interval_secs: default_summary_interval_secs(),
            periodic_summary_ms: default_periodic_summary_ms(),
            max_tool_result_chars: default_max_tool_result_chars(),
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

fn default_max_act_chars() -> usize {
    500
}

fn default_show_act_phase() -> bool {
    true
}

fn default_act_emoji() -> String {
    "⚡".to_string()
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

fn default_periodic_summary_ms() -> u64 {
    3000
}

fn default_max_tool_result_chars() -> usize {
    500
}

fn default_ack_placeholder_text() -> String {
    "已收到，开始处理。处理时间较长时我会定期同步进展。".to_string()
}

fn default_busy_text() -> String {
    "上一个请求还在处理中，请稍后再发新消息。".to_string()
}

fn default_summary_interval_secs() -> u64 {
    300
}
