//! Teloxide message sender implementation

use async_trait::async_trait;
use crate::error::BotError;
use crate::streaming::retry::{edit_message_with_retry, send_message_with_retry};
use crate::traits::MessageSender;
use teloxide::prelude::*;
use teloxide::types::{MessageId, ParseMode};

const TELEGRAM_API_RETRIES: u32 = 3;

pub struct TeloxideSender {
    bot: Bot,
}

impl TeloxideSender {
    pub fn new(bot: Bot) -> Self {
        Self { bot }
    }
}

#[async_trait]
impl MessageSender for TeloxideSender {
    async fn send_text_returning_id(&self, chat_id: i64, text: &str) -> Result<i32, BotError> {
        let msg = send_message_with_retry(
            &self.bot,
            ChatId(chat_id),
            text,
            TELEGRAM_API_RETRIES,
        )
        .await?;
        Ok(msg.id.0)
    }

    async fn send_text_with_parse_mode(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: ParseMode,
    ) -> Result<(), BotError> {
        self.bot
            .send_message(ChatId(chat_id), text)
            .parse_mode(parse_mode)
            .await
            .map_err(BotError::from)?;
        Ok(())
    }

    async fn reply_to(
        &self,
        chat_id: i64,
        _reply_to_message_id: i32,
        text: &str,
    ) -> Result<(), BotError> {
        self.bot
            .send_message(ChatId(chat_id), text)
            .await
            .map_err(BotError::from)?;
        Ok(())
    }

    async fn edit_message(
        &self,
        chat_id: i64,
        message_id: i32,
        text: &str,
    ) -> Result<(), BotError> {
        edit_message_with_retry(
            &self.bot,
            ChatId(chat_id),
            MessageId(message_id),
            text,
            TELEGRAM_API_RETRIES,
        )
        .await
    }
}
