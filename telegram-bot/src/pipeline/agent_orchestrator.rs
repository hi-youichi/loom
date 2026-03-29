use crate::config::InteractionMode;
use crate::error::BotError;
use crate::formatting::FormattedMessage;
use crate::pipeline::MessageContext;
use crate::traits::AgentRunContext;

pub async fn run_agent_for_chat(ctx: &MessageContext<'_>, prompt: &str) -> Result<(), BotError> {
    let chat_id = ctx.chat_id();
    let message_id = ctx.message_id();

    let Some(chat_run_guard) = ctx.deps.run_registry.try_acquire(chat_id).await else {
        ctx.deps
            .sender
            .send_formatted(
                chat_id,
                &FormattedMessage::markdown_v2(
                    ctx.deps.settings.streaming.busy_text.clone(),
                    ctx.deps.settings.streaming.busy_text.clone(),
                ),
            )
            .await?;
        return Ok(());
    };

    ctx.deps
        .sender
        .send_reaction(chat_id, message_id, "👌")
        .await?;

    let run_result = ctx
        .deps
        .agent
        .run(
            prompt,
            chat_id,
            AgentRunContext {
                user_message_id: Some(message_id),
                ack_message_id: None,
                interaction_mode: ctx.deps.settings.streaming.interaction_mode,
                model_override: Some(ctx.deps.model_selection.current_model(chat_id)?),
            },
        )
        .await;

    ctx.deps.metrics.increment_agent_calls();

    let mut outbound: Result<(), BotError> = Ok(());
    match run_result {
        Ok(reply) => {
            if !reply.trim().is_empty() {
                let skip_final_send = ctx.deps.settings.streaming.interaction_mode
                    == InteractionMode::Streaming
                    && ctx.deps.settings.streaming.show_act_phase;
                if !skip_final_send {
                    outbound = ctx
                        .deps
                        .sender
                        .send_formatted(chat_id, &FormattedMessage::markdown_v2(reply.clone(), reply))
                        .await;
                }
            }
        }
        Err(e) => {
            tracing::error!("Agent error: {}", e);
            ctx.deps.metrics.increment_agent_failures();
            let _ = ctx
                .deps
                .sender
                .send_formatted(
                    chat_id,
                    &FormattedMessage::markdown_v2(
                        format!("Error: {}", e),
                        format!("Error: {}", e),
                    ),
                )
                .await;
        }
    }
    chat_run_guard.release().await;
    outbound
}
