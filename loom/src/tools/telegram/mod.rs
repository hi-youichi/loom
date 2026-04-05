//! Telegram tools: send messages, polls, and files via Telegram Bot API.
//!
//! These tools allow the agent to interact with Telegram directly.

mod send_message;
mod send_poll;
mod send_document;

pub use send_message::{TelegramSendMessageTool, TOOL_TELEGRAM_SEND_MESSAGE};
pub use send_poll::{TelegramSendPollTool, TOOL_TELEGRAM_SEND_POLL};
pub use send_document::{TelegramSendDocumentTool, TOOL_TELEGRAM_SEND_DOCUMENT};

use std::sync::Arc;
use async_trait::async_trait;
use serde::Deserialize;

/// Trait for executing Telegram API calls.
/// Implemented by the telegram-bot crate and injected at runtime.
#[async_trait]
pub trait TelegramApi: Send + Sync {
    /// Send a text message to a chat.
    async fn send_message(&self, chat_id: i64, text: &str, parse_mode: Option<&str>) -> Result<i32, String>;
    
    /// Send a poll to a chat.
    async fn send_poll(
        &self,
        chat_id: i64,
        question: &str,
        options: Vec<String>,
        is_anonymous: bool,
        allows_multiple_answers: bool,
    ) -> Result<i32, String>;
    
    /// Send a document to a chat.
    async fn send_document(&self, chat_id: i64, file_path: &str, caption: Option<&str>) -> Result<i32, String>;
}

/// Global Telegram API instance.
/// Set once at bot startup.
static TELEGRAM_API: std::sync::RwLock<Option<Arc<dyn TelegramApi>>> = std::sync::RwLock::new(None);

pub fn set_telegram_api(api: Arc<dyn TelegramApi>) {
    let mut lock = TELEGRAM_API.write().unwrap();
    *lock = Some(api);
}

pub fn get_telegram_api() -> Option<Arc<dyn TelegramApi>> {
    TELEGRAM_API.read().unwrap().clone()
}

/// Common parameters for Telegram tools.
#[derive(Debug, Deserialize)]
pub struct TelegramToolParams {
    /// Target chat ID (defaults to current chat if not specified)
    pub chat_id: Option<i64>,
}
