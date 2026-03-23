//! Message routing and command handling
//!
//! Provides functions for routing incoming messages to appropriate handlers.

use crate::config::Settings;
use crate::download::{download_photo, download_document, DownloadConfig};
use crate::error::BotError;
use crate::download::{is_bot_mentioned, is_reply_to_bot};
use crate::handler_deps::HandlerDeps;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::Message;

/// Core message dispatch using injected dependencies (used in production and in tests with mocks).
pub async fn handle_message_with_deps(deps: &HandlerDeps, msg: &Message) -> Result<(), BotError> {
    let message_id = msg.id;
    let chat_id = msg.chat.id;

    tracing::info!("Message #{} in chat {}", message_id, chat_id);

    let chat_id_numeric = chat_id.0;

    if let Err(e) = tokio::fs::create_dir_all(&deps.settings.download_dir).await {
        tracing::error!("Failed to create download directory: {}", e);
    }

    match &msg.kind {
        teloxide::types::MessageKind::Common(_) => {
            if let Some(text) = msg.text() {
                tracing::info!("Text: {}", text);

                if text.trim() == "/reset" || text.trim().starts_with("/reset ") {
                    let thread_id = format!("telegram_{}", chat_id_numeric);
                    match deps.session.reset(&thread_id).await {
                        Ok(count) => {
                            deps.sender
                                .send_text(
                                    chat_id_numeric,
                                    &format!("🔄 Session reset! Deleted {} checkpoints.", count),
                                )
                                .await?;
                        }
                        Err(e) => {
                            tracing::error!("Failed to reset session: {}", e);
                            deps.sender
                                .send_text(
                                    chat_id_numeric,
                                    &format!("❌ Reset failed: {}", e),
                                )
                                .await?;
                        }
                    }
                    return Ok(());
                }

                if text.trim() == "/status" {
                    deps.sender
                        .send_text(chat_id_numeric, "✅ Bot is running!")
                        .await?;
                    return Ok(());
                }

                let should_respond = is_bot_mentioned(msg, &deps.bot_username)
                    || is_reply_to_bot(msg, &deps.bot_username);

                if deps.settings.only_respond_when_mentioned && !should_respond {
                    tracing::debug!("Ignoring message (bot not mentioned and not a reply)");
                    return Ok(());
                }

                let clean_text = if !deps.bot_username.is_empty() {
                    let mention = format!("@{} ", deps.bot_username);
                    text.replace(&mention, "")
                        .replace(&format!("@{}", deps.bot_username), "")
                } else {
                    text.to_string()
                };

                let prompt = if let Some(replied_msg) = msg.reply_to_message() {
                    if let Some(replied_text) = replied_msg.text() {
                        format!(
                            "[Replying to this message]:\n{}\n\n[User's reply]:\n{}",
                            replied_text, clean_text
                        )
                    } else {
                        clean_text.clone()
                    }
                } else {
                    clean_text.clone()
                };

                tracing::info!("Agent prompt:\n{}", prompt);

                match deps
                    .agent
                    .run(&prompt, chat_id_numeric, Some(message_id.0))
                    .await
                {
                    Ok(_reply) => {}
                    Err(e) => {
                        tracing::error!("Agent error: {}", e);
                        let _ = deps
                            .sender
                            .send_text(chat_id_numeric, &format!("Error: {}", e))
                            .await;
                    }
                }
            }

            if let Some(photos) = msg.photo() {
                match deps
                    .downloader
                    .download_photo(chat_id_numeric, message_id.0, photos)
                    .await
                {
                    Ok((path, metadata)) => {
                        tracing::info!(?metadata, "Photo downloaded");
                        deps.sender
                            .send_text(
                                chat_id_numeric,
                                &format!("📷 图片已保存: {:?}", path),
                            )
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download photo: {}", e);
                        deps.sender
                            .send_text(
                                chat_id_numeric,
                                &format!("❌ 下载失败: {}", e),
                            )
                            .await?;
                    }
                }
            }

            if let Some(doc) = msg.document() {
                match deps
                    .downloader
                    .download_document(chat_id_numeric, message_id.0, doc)
                    .await
                {
                    Ok((path, metadata)) => {
                        tracing::info!(?metadata, "Document downloaded");
                        deps.sender
                            .send_text(
                                chat_id_numeric,
                                &format!("📁 文件已保存: {:?}", path),
                            )
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download document: {}", e);
                        deps.sender
                            .send_text(
                                chat_id_numeric,
                                &format!("❌ 下载失败: {}", e),
                            )
                            .await?;
                    }
                }
            }

            if let Some(video) = msg.video() {
                match deps
                    .downloader
                    .download_video(chat_id_numeric, message_id.0, video)
                    .await
                {
                    Ok((path, metadata)) => {
                        tracing::info!(?metadata, "Video downloaded");
                        deps.sender
                            .send_text(
                                chat_id_numeric,
                                &format!("🎬 视频已保存: {:?}", path),
                            )
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download video: {}", e);
                        deps.sender
                            .send_text(
                                chat_id_numeric,
                                &format!("❌ 下载失败: {}", e),
                            )
                            .await?;
                    }
                }
            }
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
) -> Result<(), BotError> {
    let deps = HandlerDeps::production(bot, settings, bot_username);
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
