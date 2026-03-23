//! Teloxide message sender implementation

use async_trait::async_trait;
use teloxide::prelude::*;
use teloxide::types::{ParseMode, MessageId};
use crate::traits::MessageSender;
use crate::error::BotError;

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
        let msg = self
            .bot
            .send_message(ChatId(chat_id), text)
            .await
            .map_err(BotError::from)?;
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
        self.bot
            .edit_message_text(ChatId(chat_id), MessageId(message_id), text)
            .await
            .map_err(BotError::from)?;
        Ok(())
    }
}
