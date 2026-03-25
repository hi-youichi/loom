//! Message routing: teloxide entrypoints delegate to [`crate::pipeline`].

use crate::config::Settings;
use crate::download::{download_photo, download_document, DownloadConfig};
use crate::error::BotError;
use crate::handler_deps::ChatRunRegistry;
use crate::handler_deps::HandlerDeps;
use crate::pipeline::{handle_common_message, MessageContext};
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::Message;

/// Core message dispatch using injected dependencies (used in production and in tests with mocks).
pub async fn handle_message_with_deps(deps: &HandlerDeps, msg: &Message) -> Result<(), BotError> {
    let message_id = msg.id;
    let chat_id = msg.chat.id;

    tracing::info!("Message #{} in chat {}", message_id, chat_id);

    match &msg.kind {
        teloxide::types::MessageKind::Common(_) => {
            let ctx = MessageContext::new(deps, msg);
            handle_common_message(&ctx).await?;
        }

        _ => {
            tracing::debug!("Unhandled message kind: {:?}", msg.kind);
        }
    }

    Ok(())
}

/// Default message handler (long polling): builds production [`HandlerDeps`] then dispatches.
pub async fn default_handler(
    bot: Bot,
    msg: Message,
    settings: Arc<Settings>,
    bot_username: Arc<String>,
    run_registry: Arc<ChatRunRegistry>,
) -> Result<(), BotError> {
    let deps = HandlerDeps::production(bot, settings, bot_username, run_registry);
    handle_message_with_deps(&deps, &msg).await
}

/// Create a handler with custom download configuration
pub fn create_handler_with_config(config: Arc<DownloadConfig>) -> impl Fn(Bot, Message) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), BotError>> + Send>> {
    move |bot: Bot, msg: Message| {
        let config = config.clone();
        Box::pin(async move {
            let chat_id = msg.chat.id;
            let message_id = msg.id;

            if let Err(e) = config.init().await {
                tracing::error!("Failed to create download directory: {}", e);
            }

            if let Some(photos) = msg.photo() {
                match download_photo(&bot, photos, &config, chat_id.0, message_id.0).await {
                    Ok((path, _metadata)) => {
                        bot.send_message(chat_id, format!("📷 图片已保存: {:?}", path))
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download photo: {}", e);
                    }
                }
            }

            if let Some(doc) = msg.document() {
                match download_document(&bot, doc, &config, chat_id.0, message_id.0).await {
                    Ok((path, _metadata)) => {
                        bot.send_message(chat_id, format!("📁 文件已保存: {:?}", path))
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download document: {}", e);
                    }
                }
            }

            if let Some(text) = msg.text() {
                bot.send_message(chat_id, format!("收到: {}", text)).await?;
            }

            Ok(())
        })
    }
}
