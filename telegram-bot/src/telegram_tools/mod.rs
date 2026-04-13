//! Telegram API implementation for loom tools.
//!
//! Implements the `TelegramApi` trait from loom and registers it globally.

use async_trait::async_trait;
use loom::tools::TelegramApi;
use teloxide::prelude::*;
use teloxide::types::{InputFile, ParseMode};

/// Teloxide-based implementation of TelegramApi.
pub struct TeloxideTelegramApi {
    bot: Bot,
}

impl TeloxideTelegramApi {
    pub fn new(bot: Bot) -> Self {
        Self { bot }
    }
}

#[async_trait]
impl TelegramApi for TeloxideTelegramApi {
    async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<&str>,
    ) -> Result<i32, String> {
        let mut request = self.bot.send_message(ChatId(chat_id), text);

        if let Some(mode) = parse_mode {
            request = match mode {
                "MarkdownV2" => request.parse_mode(ParseMode::MarkdownV2),
                "HTML" => request.parse_mode(ParseMode::Html),
                _ => request,
            };
        }

        let message = request
            .await
            .map_err(|e| format!("Telegram API error: {}", e))?;

        Ok(message.id.0)
    }

    async fn send_poll(
        &self,
        chat_id: i64,
        question: &str,
        options: Vec<String>,
        is_anonymous: bool,
        allows_multiple_answers: bool,
    ) -> Result<i32, String> {
        let message = self
            .bot
            .send_poll(ChatId(chat_id), question, options)
            .is_anonymous(is_anonymous)
            .allows_multiple_answers(allows_multiple_answers)
            .await
            .map_err(|e| format!("Telegram API error: {}", e))?;

        Ok(message.id.0)
    }

    async fn send_document(
        &self,
        chat_id: i64,
        file_path: &str,
        caption: Option<&str>,
    ) -> Result<i32, String> {
        let file = InputFile::file(file_path);
        let mut request = self.bot.send_document(ChatId(chat_id), file);

        if let Some(cap) = caption {
            request = request.caption(cap);
        }

        let message = request
            .await
            .map_err(|e| format!("Telegram API error: {}", e))?;

        Ok(message.id.0)
    }
}

/// Initialize the Telegram API for loom tools.
pub fn init_telegram_api(bot: Bot) {
    let api = std::sync::Arc::new(TeloxideTelegramApi::new(bot));
    loom::tools::set_telegram_api(api);
}
