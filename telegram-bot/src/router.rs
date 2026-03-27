//! Message routing: teloxide entrypoints delegate to [`crate::pipeline`].

use crate::config::Settings;
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


