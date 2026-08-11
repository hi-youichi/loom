//! Teloxide message sender implementation

use async_trait::async_trait;
use teloxide::prelude::*;
use teloxide::types::{MessageId, ParseMode, ReactionType};

use crate::constants::retry::MAX_RETRIES as TELEGRAM_API_RETRIES;
use crate::constants::telegram::MESSAGE_MAX_CHARS as TELEGRAM_MESSAGE_MAX_CHARS;
use crate::error::BotError;
use crate::formatting::{markdown_to_telegram_v2, FormattedMessage};
use crate::streaming::retry::{
    edit_formatted_message_with_retry, edit_message_with_retry, send_formatted_message_with_retry,
    send_message_with_retry,
};
use crate::traits::MessageSender;
use crate::utils::{split_text_for_telegram, truncate_text};

fn preview_text(text: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 240;
    let mut preview = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= MAX_PREVIEW_CHARS {
            preview.push_str("...");
            break;
        }
        preview.push(ch);
    }
    preview.replace('\n', "\\n")
}

const EDIT_TRUNCATION_NOTICE: &str = "\n\n[truncated: exceeds Telegram edit limit]";

fn exceeds_telegram_limit(text: &str) -> bool {
    text.chars().count() > TELEGRAM_MESSAGE_MAX_CHARS
}

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
        let chunks = split_text_for_telegram(text, TELEGRAM_MESSAGE_MAX_CHARS);
        tracing::debug!(
            chat_id,
            text_len = text.chars().count(),
            chunk_count = chunks.len(),
            text_preview = %preview_text(text),
            "sending plain telegram message"
        );

        let mut first_message_id: Option<i32> = None;
        for chunk in chunks {
            let msg =
                send_message_with_retry(&self.bot, ChatId(chat_id), &chunk, TELEGRAM_API_RETRIES)
                    .await?;
            if first_message_id.is_none() {
                first_message_id = Some(msg.id.0);
            }
        }

        first_message_id
            .ok_or_else(|| BotError::Unknown("cannot send empty telegram chunk list".to_string()))
    }

    async fn send_text_with_parse_mode(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: ParseMode,
    ) -> Result<(), BotError> {
        tracing::debug!(
            chat_id,
            ?parse_mode,
            text_len = text.chars().count(),
            text_preview = %preview_text(text),
            "sending telegram message with parse mode"
        );
        if exceeds_telegram_limit(text) {
            tracing::warn!(
                chat_id,
                text_len = text.chars().count(),
                max_chars = TELEGRAM_MESSAGE_MAX_CHARS,
                "parse-mode message exceeded telegram limit, fallback to plain chunked sending"
            );
            self.send_text(chat_id, text).await?;
            return Ok(());
        }

        self.bot
            .send_message(ChatId(chat_id), text)
            .parse_mode(parse_mode)
            .await
            .map_err(BotError::from)?;
        Ok(())
    }

    async fn send_formatted_returning_id(
        &self,
        chat_id: i64,
        msg: &FormattedMessage,
    ) -> Result<i32, BotError> {
        tracing::debug!(
            chat_id,
            parse_mode = ?msg.parse_mode,
            text_len = msg.text.chars().count(),
            fallback_len = msg.plain_text_fallback.chars().count(),
            text_preview = %preview_text(&msg.text),
            fallback_preview = %preview_text(&msg.plain_text_fallback),
            "sending formatted telegram message"
        );
        match msg.parse_mode {
            Some(parse_mode) => match send_formatted_message_with_retry(
                &self.bot,
                ChatId(chat_id),
                &msg.text,
                parse_mode,
                TELEGRAM_API_RETRIES,
            )
            .await
            {
                Ok(message) => Ok(message.id.0),
                Err(e) => {
                    tracing::warn!(error = %e, "formatted telegram message failed, falling back to plain text");
                    self.send_text_returning_id(chat_id, &msg.plain_text_fallback)
                        .await
                }
            },
            None => self.send_text_returning_id(chat_id, &msg.text).await,
        }
    }

    async fn send_formatted(&self, chat_id: i64, msg: &FormattedMessage) -> Result<(), BotError> {
        tracing::debug!(
            chat_id,
            parse_mode = ?msg.parse_mode,
            text_len = msg.text.chars().count(),
            fallback_len = msg.plain_text_fallback.chars().count(),
            text_preview = %preview_text(&msg.text),
            fallback_preview = %preview_text(&msg.plain_text_fallback),
            "sending formatted telegram message without id"
        );
        if exceeds_telegram_limit(&msg.text) || exceeds_telegram_limit(&msg.plain_text_fallback) {
            tracing::warn!(
                chat_id,
                text_len = msg.text.chars().count(),
                fallback_len = msg.plain_text_fallback.chars().count(),
                max_chars = TELEGRAM_MESSAGE_MAX_CHARS,
                "formatted message exceeded telegram limit, fallback to plain chunked sending"
            );
            self.send_text(chat_id, &msg.plain_text_fallback).await?;
            return Ok(());
        }

        match msg.parse_mode {
            Some(parse_mode) => match self
                .bot
                .send_message(ChatId(chat_id), &msg.text)
                .parse_mode(parse_mode)
                .await
            {
                Ok(_) => Ok(()),
                Err(e) => {
                    tracing::warn!(error = %e, "formatted telegram message failed, falling back to plain text");
                    self.bot
                        .send_message(ChatId(chat_id), &msg.plain_text_fallback)
                        .await
                        .map_err(BotError::from)?;
                    Ok(())
                }
            },
            None => self
                .bot
                .send_message(ChatId(chat_id), &msg.text)
                .await
                .map(|_| ())
                .map_err(BotError::from),
        }
    }

    async fn reply_to(
        &self,
        chat_id: i64,
        _reply_to_message_id: i32,
        text: &str,
    ) -> Result<(), BotError> {
        let converted = markdown_to_telegram_v2(text);
        tracing::debug!(
            chat_id,
            parse_mode = ?ParseMode::MarkdownV2,
            text_len = text.chars().count(),
            converted_len = converted.chars().count(),
            text_preview = %preview_text(text),
            converted_preview = %preview_text(&converted),
            "replying with markdown telegram message"
        );

        match self
            .bot
            .send_message(ChatId(chat_id), converted)
            .parse_mode(ParseMode::MarkdownV2)
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::warn!(error = %e, "markdown reply failed, falling back to plain text");
                self.bot
                    .send_message(ChatId(chat_id), text)
                    .await
                    .map_err(BotError::from)?;
                Ok(())
            }
        }
    }

    async fn edit_message(
        &self,
        chat_id: i64,
        message_id: i32,
        text: &str,
    ) -> Result<(), BotError> {
        let text_for_edit = if exceeds_telegram_limit(text) {
            let notice_chars = EDIT_TRUNCATION_NOTICE.chars().count();
            let allowed = TELEGRAM_MESSAGE_MAX_CHARS.saturating_sub(notice_chars);
            format!("{}{}", truncate_text(text, allowed), EDIT_TRUNCATION_NOTICE)
        } else {
            text.to_string()
        };

        tracing::debug!(
            chat_id,
            message_id,
            text_len = text_for_edit.chars().count(),
            text_preview = %preview_text(&text_for_edit),
            "editing plain telegram message"
        );
        edit_message_with_retry(
            &self.bot,
            ChatId(chat_id),
            MessageId(message_id),
            &text_for_edit,
            TELEGRAM_API_RETRIES,
        )
        .await
    }

    async fn edit_formatted(
        &self,
        chat_id: i64,
        message_id: i32,
        msg: &FormattedMessage,
    ) -> Result<(), BotError> {
        tracing::debug!(
            chat_id,
            message_id,
            parse_mode = ?msg.parse_mode,
            text_len = msg.text.chars().count(),
            fallback_len = msg.plain_text_fallback.chars().count(),
            text_preview = %preview_text(&msg.text),
            fallback_preview = %preview_text(&msg.plain_text_fallback),
            "editing formatted telegram message"
        );
        match msg.parse_mode {
            Some(parse_mode) => match edit_formatted_message_with_retry(
                &self.bot,
                ChatId(chat_id),
                MessageId(message_id),
                &msg.text,
                parse_mode,
                TELEGRAM_API_RETRIES,
            )
            .await
            {
                Ok(()) => Ok(()),
                Err(e) => {
                    tracing::warn!(error = %e, "formatted telegram edit failed, falling back to plain text");
                    self.edit_message(chat_id, message_id, &msg.plain_text_fallback)
                        .await
                }
            },
            None => self.edit_message(chat_id, message_id, &msg.text).await,
        }
    }

    async fn send_reaction(
        &self,
        chat_id: i64,
        message_id: i32,
        emoji: &str,
    ) -> Result<(), BotError> {
        self.bot
            .set_message_reaction(ChatId(chat_id), MessageId(message_id))
            .reaction(vec![ReactionType::Emoji {
                emoji: emoji.to_string(),
            }])
            .await
            .map_err(BotError::from)?;
        Ok(())
    }
}
