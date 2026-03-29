//! Retry mechanism for Telegram API calls
//!
//! Provides automatic retry for transient network failures with
//! exponential backoff and jitter.

use crate::error::{BotError, Result};
use rand::Rng;
use teloxide::prelude::*;
use teloxide::types::{Message, MessageId, ParseMode};
use std::time::Duration;

const BASE_DELAY: Duration = Duration::from_secs(1);
const MAX_DELAY: Duration = Duration::from_secs(30);
const BACKOFF_FACTOR: u32 = 2;
const JITTER_PERCENT: f64 = 0.25;
const DEFAULT_RETRY_AFTER_SECS: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetryKind {
    Transient,
    RateLimited,
    Fatal,
}

fn classify_error(error: &teloxide::RequestError) -> RetryKind {
    match error {
        teloxide::RequestError::RetryAfter(_) => RetryKind::RateLimited,
        teloxide::RequestError::Network(_) => RetryKind::Transient,
        teloxide::RequestError::InvalidJson { .. } => RetryKind::Fatal,
        teloxide::RequestError::Api(_) => RetryKind::Fatal,
        teloxide::RequestError::Io(_) => RetryKind::Fatal,
        teloxide::RequestError::MigrateToChatId(_) => RetryKind::Fatal,
    }
}

fn retry_after_secs(error: &teloxide::RequestError) -> Option<u64> {
    match error {
        teloxide::RequestError::RetryAfter(secs) => Some(secs.seconds() as u64),
        _ => None,
    }
}

fn backoff_duration(attempt: u32) -> Duration {
    let base_secs = BASE_DELAY.as_secs_f64() * (BACKOFF_FACTOR as f64).powi(attempt as i32);
    let capped = base_secs.min(MAX_DELAY.as_secs_f64());
    let mut rng = rand::thread_rng();
    let jitter = capped * JITTER_PERCENT * (rng.gen::<f64>() * 2.0 - 1.0);
    let final_secs = (capped + jitter).max(0.1);
    Duration::from_secs_f64(final_secs)
}

fn fallback_error(last_error: Option<teloxide::RequestError>) -> BotError {
    match last_error {
        Some(e) => BotError::Network(e),
        None => BotError::Config("retry exhausted with no recorded error".into()),
    }
}

async fn sleep_for_kind(kind: RetryKind, last_error: &teloxide::RequestError, attempt: u32) {
    match kind {
        RetryKind::Fatal => {}
        RetryKind::RateLimited => {
            let secs = retry_after_secs(last_error).unwrap_or(DEFAULT_RETRY_AFTER_SECS);
            tracing::info!(secs, "rate limited, sleeping");
            tokio::time::sleep(Duration::from_secs(secs)).await;
        }
        RetryKind::Transient => {
            let delay = backoff_duration(attempt);
            tracing::info!(?delay, "backing off");
            tokio::time::sleep(delay).await;
        }
    }
}

pub async fn send_message_with_retry(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    text: &str,
    max_retries: u32,
) -> Result<Message> {
    let mut attempts = 0u32;
    #[allow(clippy::assigning_clones, unused_assignments)]
    #[allow(unused_assignments)]
    let mut last_error: Option<teloxide::RequestError> = None;

    loop {
        match bot.send_message(chat_id, text).await {
            Ok(msg) => return Ok(msg),
            Err(e) => {
                let kind = classify_error(&e);
                tracing::warn!(attempt = attempts + 1, max = max_retries, kind = ?kind, error = %e, "send_message failed");
                last_error = Some(e);
                attempts += 1;
                if attempts >= max_retries || kind == RetryKind::Fatal {
                    break;
                }
                sleep_for_kind(kind, last_error.as_ref().expect("assigned above"), attempts).await;
            }
        }
    }

    Err(fallback_error(last_error))
}

pub async fn edit_message_with_retry(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    message_id: MessageId,
    text: &str,
    max_retries: u32,
) -> Result<()> {
    let mut attempts = 0u32;
    #[allow(unused_assignments)]
    let mut last_error: Option<teloxide::RequestError> = None;

    loop {
        match bot.edit_message_text(chat_id, message_id, text).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                let kind = classify_error(&e);
                tracing::warn!(attempt = attempts + 1, max = max_retries, kind = ?kind, error = %e, "edit_message failed");
                last_error = Some(e);
                attempts += 1;
                if attempts >= max_retries || kind == RetryKind::Fatal {
                    break;
                }
                sleep_for_kind(kind, last_error.as_ref().expect("assigned above"), attempts).await;
            }
        }
    }

    Err(fallback_error(last_error))
}

pub async fn send_formatted_message_with_retry(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    text: &str,
    parse_mode: ParseMode,
    max_retries: u32,
) -> Result<Message> {
    let mut attempts = 0u32;
    #[allow(unused_assignments)]
    let mut last_error: Option<teloxide::RequestError> = None;

    loop {
        match bot.send_message(chat_id, text).parse_mode(parse_mode).await {
            Ok(msg) => return Ok(msg),
            Err(e) => {
                let kind = classify_error(&e);
                tracing::warn!(attempt = attempts + 1, max = max_retries, kind = ?kind, error = %e, "send_formatted_message failed");
                last_error = Some(e);
                attempts += 1;
                if attempts >= max_retries || kind == RetryKind::Fatal {
                    break;
                }
                sleep_for_kind(kind, last_error.as_ref().expect("assigned above"), attempts).await;
            }
        }
    }

    Err(fallback_error(last_error))
}

pub async fn edit_formatted_message_with_retry(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    message_id: MessageId,
    text: &str,
    parse_mode: ParseMode,
    max_retries: u32,
) -> Result<()> {
    let mut attempts = 0u32;
    #[allow(unused_assignments)]
    let mut last_error: Option<teloxide::RequestError> = None;

    loop {
        match bot.edit_message_text(chat_id, message_id, text).parse_mode(parse_mode).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                let kind = classify_error(&e);
                tracing::warn!(attempt = attempts + 1, max = max_retries, kind = ?kind, error = %e, "edit_formatted_message failed");
                last_error = Some(e);
                attempts += 1;
                if attempts >= max_retries || kind == RetryKind::Fatal {
                    break;
                }
                sleep_for_kind(kind, last_error.as_ref().expect("assigned above"), attempts).await;
            }
        }
    }

    Err(fallback_error(last_error))
}
