//! Retry mechanism for Telegram API calls
//!
//! Provides automatic retry for transient network failures.

use crate::error::{BotError, Result};
use teloxide::prelude::*;
use teloxide::types::{MessageId, Message};
use std::time::Duration;

/// Send a message with automatic retry on failure
pub async fn send_message_with_retry(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    text: &str,
    max_retries: u32,
) -> Result<Message> {
    let mut attempts = 0;
    let mut last_error = None;
    
    while attempts < max_retries {
        match bot.send_message(chat_id, text).await {
            Ok(msg) => return Ok(msg),
            Err(e) => {
                attempts += 1;
                last_error = Some(e);
                tracing::warn!(
                    "Failed to send message (attempt {}/{}): {}",
                    attempts, max_retries, last_error.as_ref().unwrap()
                );
                if attempts < max_retries {
                    tokio::time::sleep(Duration::from_millis(100 * attempts as u64)).await;
                }
            }
        }
    }
    
    Err(BotError::Network(last_error.unwrap()))
}

/// Edit a message with automatic retry on failure
pub async fn edit_message_with_retry(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    message_id: MessageId,
    text: &str,
    max_retries: u32,
) -> Result<()> {
    let mut attempts = 0;
    let mut last_error = None;
    
    while attempts < max_retries {
        match bot.edit_message_text(chat_id, message_id, text).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                attempts += 1;
                last_error = Some(e);
                tracing::warn!(
                    "Failed to edit message (attempt {}/{}): {}",
                    attempts, max_retries, last_error.as_ref().unwrap()
                );
                if attempts < max_retries {
                    tokio::time::sleep(Duration::from_millis(100 * attempts as u64)).await;
                }
            }
        }
    }
    
    Err(BotError::Network(last_error.unwrap()))
}
