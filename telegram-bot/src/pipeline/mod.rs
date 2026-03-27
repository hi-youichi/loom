//! Message processing pipeline: download dir, text flow (commands → mention gate → agent), media.
//!
//! Keeps [`crate::router::handle_message_with_deps`] thin by centralizing common-message logic here.

use crate::command::{CommandContext, CommandDispatcher};
use crate::config::InteractionMode;
use crate::download::{is_bot_mentioned, is_reply_to_bot};
use crate::error::BotError;
use crate::handler_deps::HandlerDeps;
use crate::traits::AgentRunContext;
use teloxide::types::Message;

/// Per-message view with injected [`HandlerDeps`].
pub struct MessageContext<'a> {
    pub deps: &'a HandlerDeps,
    pub msg: &'a Message,
}

impl<'a> MessageContext<'a> {
    pub fn new(deps: &'a HandlerDeps, msg: &'a Message) -> Self {
        Self { deps, msg }
    }

    pub fn chat_id(&self) -> i64 {
        self.msg.chat.id.0
    }

    pub fn message_id(&self) -> i32 {
        self.msg.id.0
    }
}

pub async fn ensure_download_dir(deps: &HandlerDeps) {
    if let Err(e) = tokio::fs::create_dir_all(&deps.settings.download_dir).await {
        tracing::error!("Failed to create download directory: {}", e);
    }
}

fn strip_bot_mention(text: &str, bot_username: &str) -> String {
    if bot_username.is_empty() {
        return text.to_string();
    }
    let mention = format!("@{} ", bot_username);
    text.replace(&mention, "")
        .replace(&format!("@{}", bot_username), "")
}

fn build_prompt_with_reply(msg: &Message, clean_text: &str) -> String {
    if let Some(replied_msg) = msg.reply_to_message() {
        if let Some(replied_text) = replied_msg.text() {
            return format!(
                "[Replying to this message]:\n{}\n\n[User's reply]:\n{}",
                replied_text, clean_text
            );
        }
    }
    clean_text.to_string()
}

async fn run_agent_for_chat(ctx: &MessageContext<'_>, prompt: &str) -> Result<(), BotError> {
    let chat_id = ctx.chat_id();
    let message_id = ctx.message_id();

    let Some(chat_run_guard) = ctx.deps.run_registry.try_acquire(chat_id).await else {
        ctx.deps
            .sender
            .send_text(chat_id, &ctx.deps.settings.streaming.busy_text)
            .await?;
        return Ok(());
    };

    let ack_message_id = if ctx.deps.settings.streaming.interaction_mode == InteractionMode::PeriodicSummary
    {
        Some(
            ctx.deps
                .sender
                .send_text_returning_id(
                    chat_id,
                    &ctx.deps.settings.streaming.ack_placeholder_text,
                )
                .await?,
        )
    } else {
        None
    };

    let run_result = ctx
        .deps
        .agent
        .run(
            prompt,
            chat_id,
            AgentRunContext {
                user_message_id: Some(message_id),
                ack_message_id,
                interaction_mode: ctx.deps.settings.streaming.interaction_mode,
            },
        )
        .await;

    let mut outbound: Result<(), BotError> = Ok(());
    match run_result {
        Ok(reply) => {
            if !reply.trim().is_empty() {
                let skip_final_send = ctx.deps.settings.streaming.interaction_mode
                    == InteractionMode::Streaming
                    && (ctx.deps.settings.streaming.show_act_phase
                        || ctx.deps.settings.streaming.show_think_phase);
                if !skip_final_send {
                    outbound = ctx.deps.sender.send_text(chat_id, &reply).await;
                }
            }
        }
        Err(e) => {
            tracing::error!("Agent error: {}", e);
            let _ = ctx
                .deps
                .sender
                .send_text(chat_id, &format!("Error: {}", e))
                .await;
        }
    }
    chat_run_guard.release().await;
    outbound
}

async fn handle_media_attachments(ctx: &MessageContext<'_>) -> Result<(), BotError> {
    let chat_id = ctx.chat_id();
    let message_id = ctx.message_id();

    if let Some(photos) = ctx.msg.photo() {
        match ctx
            .deps
            .downloader
            .download_photo(chat_id, message_id, photos)
            .await
        {
            Ok((_path, metadata)) => {
                tracing::info!(?metadata, "Photo downloaded");
            }
            Err(e) => {
                tracing::error!("Failed to download photo: {}", e);
            }
        }
    }

    if let Some(doc) = ctx.msg.document() {
        match ctx
            .deps
            .downloader
            .download_document(chat_id, message_id, doc)
            .await
        {
            Ok((_path, metadata)) => {
                tracing::info!(?metadata, "Document downloaded");
            }
            Err(e) => {
                tracing::error!("Failed to download document: {}", e);
            }
        }
    }

    if let Some(video) = ctx.msg.video() {
        match ctx
            .deps
            .downloader
            .download_video(chat_id, message_id, video)
            .await
        {
            Ok((_path, metadata)) => {
                tracing::info!(?metadata, "Video downloaded");
            }
            Err(e) => {
                tracing::error!("Failed to download video: {}", e);
            }
        }
    }

    Ok(())
}

/// Full handling for [`teloxide::types::MessageKind::Common`].
pub async fn handle_common_message(ctx: &MessageContext<'_>) -> Result<(), BotError> {
    ensure_download_dir(ctx.deps).await;

    if let Some(text) = ctx.msg.text() {
        tracing::info!("Text: {}", text);

        let cmd_ctx = CommandContext {
            chat_id: ctx.chat_id(),
            deps: ctx.deps,
        };
        let dispatcher = CommandDispatcher::default();

        if let Some(result) = dispatcher.try_dispatch(&cmd_ctx, text).await {
            return result;
        }

        let should_respond = is_bot_mentioned(ctx.msg, &ctx.deps.bot_username)
            || is_reply_to_bot(ctx.msg, &ctx.deps.bot_username);

        if ctx.deps.settings.only_respond_when_mentioned && !should_respond {
            tracing::debug!("Ignoring message (bot not mentioned and not a reply)");
            return Ok(());
        }

        let clean_text = strip_bot_mention(text, &ctx.deps.bot_username);
        let prompt = build_prompt_with_reply(ctx.msg, &clean_text);
        tracing::info!("Agent prompt:\n{}", prompt);

        run_agent_for_chat(ctx, &prompt).await?;
    }

    handle_media_attachments(ctx).await?;
    Ok(())
}
