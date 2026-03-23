//! Message routing and command handling
//!
//! Provides functions for routing incoming messages to appropriate handlers.

use crate::config::Settings;
use crate::download::{DownloadConfig, download_photo, download_document, download_video};
use crate::streaming::run_loom_agent_streaming;
use crate::error::BotError;
use crate::download::{is_bot_mentioned, is_reply_to_bot, reset_session};
use crate::download::FileMetadata;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::Message;

/// Default message handler
///
/// This handler processes incoming messages and routes them to appropriate handlers.
pub async fn default_handler(
    bot: Bot, 
    msg: Message,
    settings: Arc<Settings>,
    bot_username: Arc<String>,
) -> Result<(), BotError> {
    let message_id = msg.id;
    let chat_id = msg.chat.id;
    
    tracing::info!(
        "Message #{} in chat {}",
        message_id, chat_id
    );
    
    let config = DownloadConfig::default();
    
    if let Err(e) = config.init().await {
        tracing::error!("Failed to create download directory: {}", e);
    }
    
    match &msg.kind {
        teloxide::types::MessageKind::Common(_) => {
            if let Some(text) = msg.text() {
                tracing::info!("Text: {}", text);
                
                if text.trim() == "/reset" || text.trim().starts_with("/reset ") {
                    let thread_id = format!("telegram_{}", chat_id.0);
                    match reset_session(&thread_id) {
                        Ok(count) => {
                            bot.send_message(chat_id, format!("🔄 Session reset! Deleted {} checkpoints.", count))
                                .await?;
                        }
                        Err(e) => {
                            tracing::error!("Failed to reset session: {}", e);
                            bot.send_message(chat_id, format!("❌ Reset failed: {}", e))
                                .await?;
                        }
                    }
                    return Ok(());
                }
                
                if text.trim() == "/status" {
                    bot.send_message(chat_id, "✅ Bot is running!")
                        .await?;
                    return Ok(());
                }
                
                let should_respond = is_bot_mentioned(&msg, &bot_username) || is_reply_to_bot(&msg, &bot_username);
                
                if settings.only_respond_when_mentioned && !should_respond {
                    tracing::debug!("Ignoring message (bot not mentioned and not a reply)");
                    return Ok(());
                }

                let clean_text = if !bot_username.is_empty() {
                    let mention = format!("@{} ", bot_username);
                    text.replace(&mention, "").replace(&format!("@{}", bot_username), "")
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
                
                match run_loom_agent_streaming(&prompt, chat_id.0, bot.clone(), Some(message_id.0), &settings).await {
                    Ok(_reply) => {}
                    Err(e) => {
                        tracing::error!("Agent error: {}", e);
                        let _ = bot.send_message(chat_id, format!("Error: {}", e)).await;
                    }
                }
            }
            
            if let Some(photos) = msg.photo() {
                match download_photo(&bot, photos, &config, chat_id.0, message_id.0).await {
                    Ok((path, metadata)) => {
                        tracing::info!(?metadata, "Photo downloaded");
                        bot.send_message(chat_id, format!("📷 图片已保存: {:?}", path))
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download photo: {}", e);
                        bot.send_message(chat_id, format!("❌ 下载失败: {}", e))
                            .await?;
                    }
                }
            }
            
            if let Some(doc) = msg.document() {
                match download_document(&bot, doc, &config, chat_id.0, message_id.0).await {
                    Ok((path, metadata)) => {
                        tracing::info!(?metadata, "Document downloaded");
                        bot.send_message(chat_id, format!("📁 文件已保存: {:?}", path))
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download document: {}", e);
                        bot.send_message(chat_id, format!("❌ 下载失败: {}", e))
                            .await?;
                    }
                }
            }
            
            if let Some(video) = msg.video() {
                match download_video(&bot, video, &config, chat_id.0, message_id.0).await {
                    Ok((path, metadata)) => {
                        tracing::info!(?metadata, "Video downloaded");
                        bot.send_message(chat_id, format!("🎬 视频已保存: {:?}", path))
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to download video: {}", e);
                        bot.send_message(chat_id, format!("❌ 下载失败: {}", e))
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
