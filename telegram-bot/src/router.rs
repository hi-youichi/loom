use crate::config::Settings;
use crate::error::BotError;
use crate::handler_deps::ChatRunRegistry;
use crate::handler_deps::HandlerDeps;
use crate::pipeline::{handle_common_message, MessageContext};
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::Message;

pub async fn handle_message_with_deps(deps: &HandlerDeps, msg: &Message) -> Result<(), BotError> {
    deps.metrics.increment_messages();

    let message_id = msg.id;
    let chat_id = msg.chat.id;

    tracing::info!("Message #{} in chat {}", message_id, chat_id);

    match &msg.kind {
        teloxide::types::MessageKind::Common(_) => {
            let ctx = MessageContext::new(deps, msg);
            if let Err(e) = handle_common_message(&ctx).await {
                deps.metrics.increment_failures();
                return Err(e);
            }
        }

        _ => {
            tracing::debug!("Unhandled message kind: {:?}", msg.kind);
        }
    }

    Ok(())
}

pub async fn default_handler(
    bot: Bot,
    msg: Message,
    settings: Arc<Settings>,
    bot_username: Arc<String>,
    run_registry: Arc<ChatRunRegistry>,
) -> Result<(), BotError> {
    let deps = HandlerDeps::production(bot, settings, bot_username, run_registry)?;
    handle_message_with_deps(&deps, &msg).await
}
